use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::warn;
use twilight_model::guild::audit_log::{AuditLogEntry, AuditLogEventType};
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::id::Id;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

use crate::state::AppState;
use crate::utils::{
    apply_embed_footer, apply_punishment, discord_timestamp_now, is_allowed_guild,
    is_owner_or_whitelisted, now_unix_millis, send_log_embed,
};

/// Discord Epoch (2015-01-01T00:00:00Z) in milliseconds for extracting Snowflake timestamps.
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

/// Maximum age (in milliseconds) for an AuditLogEntry to be considered freshly triggered by the current Gateway event.
const MAX_AUDIT_ENTRY_AGE_MS: u64 = 10_000;

/// Fetches recent audit log entries of a specific `action_type` for a guild.
///
/// Inputs:
/// - `http`: Shared Twilight HTTP client.
/// - `guild_id`: Target Discord Guild ID.
/// - `action_type`: The `AuditLogEventType` to filter by.
/// - `limit`: Maximum number of entries to retrieve (1..=100).
///
/// Outputs:
/// - `Option<Vec<AuditLogEntry>>`: List of audit log entries if the request succeeded.
async fn fetch_audit_log_entries(
    http: &twilight_http::Client,
    guild_id: Id<GuildMarker>,
    action_type: AuditLogEventType,
    limit: u16,
) -> Option<Vec<AuditLogEntry>> {
    let req = http.audit_log(guild_id).action_type(action_type).limit(limit).ok()?;
    let res = match req.await {
        Ok(r) => r,
        Err(e) => {
            warn!("Failed to fetch audit log ({:?}) for guild {}: {:?}", action_type, guild_id, e);
            return None;
        }
    };
    match res.model().await {
        Ok(model) => Some(model.entries),
        Err(e) => {
            warn!("Failed to deserialize audit log ({:?}) for guild {}: {:?}", action_type, guild_id, e);
            None
        }
    }
}

/// Resolves the executor `Id<UserMarker>` of a recent, un-processed audit log entry matching
/// the given `action_type` and optional `expected_target_id`.
///
/// Why this is needed:
/// Gateway events like `MemberRemove` or `MemberUpdate` also fire when members leave voluntarily
/// or update their own profile. Without checking Snowflake freshness, target ID, and deduplicating
/// `entry.id`, the bot would repeatedly attribute unrelated events to stale audit log entries.
///
/// Inputs:
/// - `state`: Shared bot application state.
/// - `guild_id`: Target Discord Guild ID.
/// - `action_type`: Expected `AuditLogEventType`.
/// - `expected_target_id`: Optional raw `u64` ID of the target entity (user, role, or channel).
///
/// Outputs:
/// - `Option<Id<UserMarker>>`: The non-bot executor user ID if a fresh, matching entry was found.
async fn resolve_valid_audit_executor(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    action_type: AuditLogEventType,
    expected_target_id: Option<u64>,
) -> Option<Id<UserMarker>> {
    let entries = fetch_audit_log_entries(&state.http, guild_id, action_type, 5).await?;
    let now_ms = now_unix_millis();

    for entry in entries {
        let entry_id_raw = entry.id.get();
        let created_ms = (entry_id_raw >> 22) + DISCORD_EPOCH_MS;

        // Ignore stale audit log entries older than MAX_AUDIT_ENTRY_AGE_MS
        if now_ms.saturating_sub(created_ms) > MAX_AUDIT_ENTRY_AGE_MS {
            continue;
        }

        // Verify target entity matches when known
        if let Some(expected_target) = expected_target_id {
            let actual_target = entry.target_id.map(|t| t.get());
            if actual_target != Some(expected_target) {
                continue;
            }
        }

        let user_id = entry.user_id?;

        // Ignore actions performed by the protection bot itself
        if let Some(bot_id) = state.get_bot_user_id() {
            if user_id == bot_id {
                return None;
            }
        }

        // Deduplicate by AuditLogEntry ID so the same entry is never counted twice
        if !state.mark_audit_entry_if_new(entry_id_raw) {
            return None;
        }

        return Some(user_id);
    }

    None
}

pub async fn handle_ban_activity(
    state: Arc<AppState>,
    guild_id: Id<GuildMarker>,
    target_user_id: Option<u64>,
) {
    let (cfg, embed_footer) = {
        let c = state.config.read();
        if !c.ban_monitoring.active || !is_allowed_guild(&c.allowed_guild_id, guild_id) {
            return;
        }
        (c.ban_monitoring.clone(), c.embed_footer.clone())
    };

    let user_id = match resolve_valid_audit_executor(
        &state,
        guild_id,
        AuditLogEventType::MemberBanAdd,
        target_user_id,
    )
    .await
    {
        Some(uid) => uid,
        None => return,
    };

    // Whitelist check
    if is_owner_or_whitelisted(&state, guild_id, user_id, None).await {
        return;
    }

    let now = Instant::now();
    let time_window = Duration::from_millis(cfg.time_frame);

    let should_punish = {
        let mut user_entry = state.ban_cache.entry(user_id).or_default();
        user_entry.push(now);
        user_entry.retain(|t| now.duration_since(*t) < time_window);
        if user_entry.len() as u64 > cfg.max_bans {
            user_entry.clear();
            true
        } else {
            false
        }
    };

    if should_punish {
        apply_punishment(
            &state.http,
            guild_id,
            user_id,
            &cfg.action,
            cfg.timeout_duration,
            "Excessive banning detected.",
        )
        .await;

        let log_embed = apply_embed_footer(
            EmbedBuilder::new()
                .title("Banning Detected")
                .color(0xFF0000)
                .description(format!(
                    "**User :** <@{}>\n**Action :** {}",
                    user_id, cfg.action
                ))
                .field(EmbedFieldBuilder::new("Detected At", discord_timestamp_now()).build()),
            &embed_footer,
        )
        .build();

        send_log_embed(&state, log_embed).await;
    }
}

pub async fn handle_kick_activity(
    state: Arc<AppState>,
    guild_id: Id<GuildMarker>,
    target_user_id: Option<u64>,
) {
    let (cfg, embed_footer) = {
        let c = state.config.read();
        if !c.kick_monitoring.active || !is_allowed_guild(&c.allowed_guild_id, guild_id) {
            return;
        }
        (c.kick_monitoring.clone(), c.embed_footer.clone())
    };

    let user_id = match resolve_valid_audit_executor(
        &state,
        guild_id,
        AuditLogEventType::MemberKick,
        target_user_id,
    )
    .await
    {
        Some(uid) => uid,
        None => return,
    };

    if is_owner_or_whitelisted(&state, guild_id, user_id, None).await {
        return;
    }

    let now = Instant::now();
    let time_window = Duration::from_millis(cfg.time_frame);

    let should_punish = {
        let mut user_entry = state.kick_cache.entry(user_id).or_default();
        user_entry.push(now);
        user_entry.retain(|t| now.duration_since(*t) < time_window);
        if user_entry.len() as u64 > cfg.max_kicks {
            user_entry.clear();
            true
        } else {
            false
        }
    };

    if should_punish {
        apply_punishment(
            &state.http,
            guild_id,
            user_id,
            &cfg.action,
            cfg.timeout_duration,
            "Excessive kicking of members detected.",
        )
        .await;

        let log_embed = apply_embed_footer(
            EmbedBuilder::new()
                .title("Member Kicking Detected")
                .color(0xFF0000)
                .description(format!(
                    "**User :** <@{}>\n**Action :** {}",
                    user_id, cfg.action
                ))
                .field(EmbedFieldBuilder::new("Detected At", discord_timestamp_now()).build()),
            &embed_footer,
        )
        .build();

        send_log_embed(&state, log_embed).await;
    }
}

pub async fn handle_bot_addition(
    state: Arc<AppState>,
    guild_id: Id<GuildMarker>,
    bot_id: Id<UserMarker>,
) {
    let (cfg, embed_footer) = {
        let c = state.config.read();
        if !c.bot_monitoring.active || !is_allowed_guild(&c.allowed_guild_id, guild_id) {
            return;
        }
        (c.bot_monitoring.clone(), c.embed_footer.clone())
    };

    let user_id = match resolve_valid_audit_executor(
        &state,
        guild_id,
        AuditLogEventType::BotAdd,
        Some(bot_id.get()),
    )
    .await
    {
        Some(uid) => uid,
        None => return,
    };

    if is_owner_or_whitelisted(&state, guild_id, user_id, None).await {
        return;
    }

    // Punish executor
    apply_punishment(
        &state.http,
        guild_id,
        user_id,
        &cfg.action,
        cfg.timeout_duration,
        "Unauthorized bot addition detected.",
    )
    .await;

    // Punish bot
    apply_punishment(
        &state.http,
        guild_id,
        bot_id,
        &cfg.bot_action,
        0,
        "Unauthorized bot addition.",
    )
    .await;

    let log_embed = apply_embed_footer(
        EmbedBuilder::new()
            .title("Added Bot Detected")
            .color(0xFF0000)
            .description(format!(
                "**User :** <@{}>\n**Bot :** <@{}>\n**User Action :** {}\n**Bot Action :** {}",
                user_id, bot_id, cfg.action, cfg.bot_action
            )),
        &embed_footer,
    )
    .build();

    send_log_embed(&state, log_embed).await;
}

pub async fn handle_role_activity(
    state: Arc<AppState>,
    guild_id: Id<GuildMarker>,
    action_str: &'static str,
    action_type: AuditLogEventType,
    target_role_id: Option<u64>,
) {
    let (cfg, embed_footer) = {
        let c = state.config.read();
        if !c.role_monitoring.active || !is_allowed_guild(&c.allowed_guild_id, guild_id) {
            return;
        }
        (c.role_monitoring.clone(), c.embed_footer.clone())
    };

    let user_id = match resolve_valid_audit_executor(&state, guild_id, action_type, target_role_id).await {
        Some(uid) => uid,
        None => return,
    };

    if is_owner_or_whitelisted(&state, guild_id, user_id, None).await {
        return;
    }

    let now = Instant::now();
    let time_window = Duration::from_millis(cfg.time_frame);

    let should_punish = {
        let mut user_entry = state.role_cache.entry(user_id).or_default();
        user_entry.push(now);
        user_entry.retain(|t| now.duration_since(*t) < time_window);
        if user_entry.len() as u64 > cfg.max_role_changes {
            user_entry.clear();
            true
        } else {
            false
        }
    };

    if should_punish {
        apply_punishment(
            &state.http,
            guild_id,
            user_id,
            &cfg.action,
            cfg.timeout_duration,
            "Excessive role creation/deletion detected.",
        )
        .await;

        let log_embed = apply_embed_footer(
            EmbedBuilder::new()
                .title(format!("Role {} Detected", action_str))
                .color(0xFF0000)
                .description(format!(
                    "**User :** <@{}>\n**Action :** {}",
                    user_id, cfg.action
                ))
                .field(EmbedFieldBuilder::new("Detected At", discord_timestamp_now()).build()),
            &embed_footer,
        )
        .build();

        send_log_embed(&state, log_embed).await;
    }
}

pub async fn handle_role_edit_activity(
    state: Arc<AppState>,
    guild_id: Id<GuildMarker>,
    action_type: AuditLogEventType,
    target_id: Option<u64>,
) {
    let (cfg, embed_footer) = {
        let c = state.config.read();
        if !c.role_edit_monitoring.active || !is_allowed_guild(&c.allowed_guild_id, guild_id) {
            return;
        }
        (c.role_edit_monitoring.clone(), c.embed_footer.clone())
    };

    let user_id = match resolve_valid_audit_executor(&state, guild_id, action_type, target_id).await {
        Some(uid) => uid,
        None => return,
    };

    if is_owner_or_whitelisted(&state, guild_id, user_id, None).await {
        return;
    }

    let now = Instant::now();
    let time_window = Duration::from_millis(cfg.time_frame);

    let should_punish = {
        let mut user_entry = state.role_edit_cache.entry(user_id).or_default();
        user_entry.push(now);
        user_entry.retain(|t| now.duration_since(*t) < time_window);
        if user_entry.len() as u64 > cfg.max_edits {
            user_entry.clear();
            true
        } else {
            false
        }
    };

    if should_punish {
        apply_punishment(
            &state.http,
            guild_id,
            user_id,
            &cfg.action,
            cfg.timeout_duration,
            "Excessive role editing detected.",
        )
        .await;

        let log_embed = apply_embed_footer(
            EmbedBuilder::new()
                .title("Role Editing Detected")
                .color(0xFF0000)
                .description(format!(
                    "**User :** <@{}>\n**Action :** {}",
                    user_id, cfg.action
                ))
                .field(EmbedFieldBuilder::new("Detected At", discord_timestamp_now()).build()),
            &embed_footer,
        )
        .build();

        send_log_embed(&state, log_embed).await;
    }
}

pub async fn handle_channel_activity(
    state: Arc<AppState>,
    guild_id: Id<GuildMarker>,
    action_str: &'static str,
    action_type: AuditLogEventType,
    target_channel_id: Option<u64>,
) {
    let (cfg, embed_footer) = {
        let c = state.config.read();
        if !c.channel_monitoring.active || !is_allowed_guild(&c.allowed_guild_id, guild_id) {
            return;
        }
        (c.channel_monitoring.clone(), c.embed_footer.clone())
    };

    let user_id = match resolve_valid_audit_executor(&state, guild_id, action_type, target_channel_id).await {
        Some(uid) => uid,
        None => return,
    };

    if is_owner_or_whitelisted(&state, guild_id, user_id, None).await {
        return;
    }

    let now = Instant::now();
    let time_window = Duration::from_millis(cfg.time_frame);

    let should_punish = {
        let mut user_entry = state.channel_cache.entry(user_id).or_default();
        user_entry.push(now);
        user_entry.retain(|t| now.duration_since(*t) < time_window);
        if user_entry.len() as u64 > cfg.max_channel_changes {
            user_entry.clear();
            true
        } else {
            false
        }
    };

    if should_punish {
        apply_punishment(
            &state.http,
            guild_id,
            user_id,
            &cfg.action,
            cfg.timeout_duration,
            "Excessive channel creation/deletion detected.",
        )
        .await;

        let log_embed = apply_embed_footer(
            EmbedBuilder::new()
                .title(format!("Channel {} Detected", action_str))
                .color(0xFF0000)
                .description(format!(
                    "**User :** <@{}>\n**Action :** {}",
                    user_id, cfg.action
                ))
                .field(EmbedFieldBuilder::new("Detected At", discord_timestamp_now()).build()),
            &embed_footer,
        )
        .build();

        send_log_embed(&state, log_embed).await;
    }
}

pub async fn handle_server_edit_activity(state: Arc<AppState>, guild_id: Id<GuildMarker>) {
    let (cfg, embed_footer) = {
        let c = state.config.read();
        if !c.server_edit_monitoring.active || !is_allowed_guild(&c.allowed_guild_id, guild_id) {
            return;
        }
        (c.server_edit_monitoring.clone(), c.embed_footer.clone())
    };

    let user_id = match resolve_valid_audit_executor(
        &state,
        guild_id,
        AuditLogEventType::GuildUpdate,
        None,
    )
    .await
    {
        Some(uid) => uid,
        None => return,
    };

    if is_owner_or_whitelisted(&state, guild_id, user_id, None).await {
        return;
    }

    let now = Instant::now();
    let time_window = Duration::from_millis(cfg.time_frame);

    let should_punish = {
        let mut user_entry = state.server_edit_cache.entry(user_id).or_default();
        user_entry.push(now);
        user_entry.retain(|t| now.duration_since(*t) < time_window);
        if user_entry.len() as u64 > cfg.max_edits {
            user_entry.clear();
            true
        } else {
            false
        }
    };

    if should_punish {
        apply_punishment(
            &state.http,
            guild_id,
            user_id,
            &cfg.action,
            cfg.timeout_duration,
            "Excessive server editing detected.",
        )
        .await;

        let log_embed = apply_embed_footer(
            EmbedBuilder::new()
                .title("Server Editing Detected")
                .color(0xFF0000)
                .description(format!(
                    "**User :** <@{}>\n**Action :** {}",
                    user_id, cfg.action
                ))
                .field(EmbedFieldBuilder::new("Detected At", discord_timestamp_now()).build()),
            &embed_footer,
        )
        .build();

        send_log_embed(&state, log_embed).await;
    }
}
