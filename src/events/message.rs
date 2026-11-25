use futures_util::future::join_all;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tracing::warn;
use twilight_model::channel::Message;
use twilight_model::guild::Permissions;
use twilight_model::http::permission_overwrite::{PermissionOverwrite, PermissionOverwriteType};
use twilight_model::id::marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker};
use twilight_model::id::Id;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

use crate::state::{AppState, HackedUserEntry};
use crate::utils::{
    apply_embed_footer, apply_punishment, discord_timestamp_now, is_allowed_guild,
    is_owner_or_whitelisted, send_log_embed,
};

static LINK_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(https?://[^\s]+|(?:[a-zA-Z0-9-]+\.)+[a-zA-Z]{2,}(?:/[^\s]*)?)")
        .expect("Invalid Regex")
});

/// Processes incoming guild messages for Hack Channel triggers, Anti-Links, and Anti-Spam.
///
/// Inputs:
/// - `state`: Shared bot application state.
/// - `message`: Incoming Discord `Message` payload.
///
/// Outputs:
/// - None.
pub async fn handle_message(state: Arc<AppState>, message: Message) {
    let guild_id = match message.guild_id {
        Some(gid) => gid,
        None => return,
    };

    // Ignore bots and webhooks to prevent self-loops or punishing other bots
    if message.author.bot || message.webhook_id.is_some() {
        return;
    }

    let author_id = message.author.id;

    // Check allowed guild
    let (allowed_guild, embed_footer) = {
        let cfg = state.config.read();
        (cfg.allowed_guild_id.clone(), cfg.embed_footer.clone())
    };
    if !is_allowed_guild(&allowed_guild, guild_id) {
        return;
    }

    // Ignore bot's own messages
    if let Some(bot_id) = state.get_bot_user_id() {
        if author_id == bot_id {
            return;
        }
    }

    let member_roles: Vec<Id<RoleMarker>> = message
        .member
        .as_ref()
        .map(|m| m.roles.clone())
        .unwrap_or_default();

    // Check if guild owner or whitelisted role holder (uses cached guild_owner_id)
    let is_exempt = is_owner_or_whitelisted(&state, guild_id, author_id, Some(&member_roles)).await;
    if is_exempt {
        return;
    }

    // 1. Hack Channel System
    let (hack_active, hack_channel_id) = {
        let cfg = state.config.read();
        (cfg.hack_mode.active, cfg.hack_mode.hack_channel_id.clone())
    };

    if hack_active && message.channel_id.get().to_string() == hack_channel_id {
        handle_hack_channel_trigger(&state, guild_id, &message, &member_roles).await;
        return;
    }

    // 2. Anti-Links System
    let antilinks_cfg = {
        let cfg = state.config.read();
        cfg.antilinks.clone()
    };

    if antilinks_cfg.active && LINK_REGEX.is_match(&message.content) {
        let mut action_taken = "Detected".to_string();

        if antilinks_cfg.delete_messages {
            if let Err(e) = state.http.delete_message(message.channel_id, message.id).await {
                warn!("Failed to delete link message {}: {:?}", message.id, e);
            }
            action_taken = "Deleted Message".to_string();
        }

        if antilinks_cfg.actions.timeout {
            apply_punishment(
                &state.http,
                guild_id,
                author_id,
                "timeout",
                antilinks_cfg.actions.timeout_duration,
                "Posting links is not allowed.",
            )
            .await;
            action_taken.push_str(", Timeout");
        }

        if antilinks_cfg.actions.kick {
            apply_punishment(
                &state.http,
                guild_id,
                author_id,
                "kick",
                0,
                "Posting links is not allowed.",
            )
            .await;
            action_taken.push_str(", Kick");
        }

        if antilinks_cfg.actions.ban {
            apply_punishment(
                &state.http,
                guild_id,
                author_id,
                "ban",
                0,
                "Posting links is not allowed.",
            )
            .await;
            action_taken.push_str(", Ban");
        }

        let log_embed = apply_embed_footer(
            EmbedBuilder::new()
                .title("Link Detection")
                .color(0xFFA500)
                .field(EmbedFieldBuilder::new("User", format!("<@{}>", author_id)).inline().build())
                .field(EmbedFieldBuilder::new("Action", action_taken).inline().build())
                .field(EmbedFieldBuilder::new("Channel", format!("<#{}>", message.channel_id)).inline().build())
                .field(EmbedFieldBuilder::new("Detected At", discord_timestamp_now()).build()),
            &embed_footer,
        )
        .build();

        send_log_embed(&state, log_embed).await;
        return;
    }

    // 3. Anti-Spam System
    let antispam_cfg = {
        let cfg = state.config.read();
        cfg.antispam.clone()
    };

    if antispam_cfg.active {
        let now = Instant::now();
        let time_window = Duration::from_millis(antispam_cfg.time);

        let mut user_entry = state.spam_cache.entry(author_id).or_default();
        user_entry.push((message.channel_id, message.id, now));

        // Retain only messages within the time window
        user_entry.retain(|(_, _, timestamp)| now.duration_since(*timestamp) < time_window);

        if user_entry.len() as u64 > antispam_cfg.message_count {
            warn!("Spam detected by user {}", author_id);
            let mut action_log = "None".to_string();

            // Snapshot messages by channel and clear user bucket immediately while holding lock
            let mut by_channel: HashMap<Id<ChannelMarker>, Vec<Id<MessageMarker>>> = HashMap::new();
            for (chan_id, msg_id, _) in user_entry.iter() {
                by_channel.entry(*chan_id).or_default().push(*msg_id);
            }
            user_entry.clear();
            drop(user_entry);

            // Delete spam messages grouped per channel
            if antispam_cfg.delete_messages {
                for (chan_id, mut msg_ids) in by_channel {
                    msg_ids.sort_unstable_by_key(|id| id.get());
                    msg_ids.dedup();

                    for chunk in msg_ids.chunks(100) {
                        if chunk.len() >= 2 {
                            match state.http.delete_messages(chan_id, chunk) {
                                Ok(req) => {
                                    if let Err(e) = req.await {
                                        warn!("Failed to bulk-delete spam messages in {}: {:?}", chan_id, e);
                                    }
                                }
                                Err(e) => warn!("Invalid bulk-delete request in {}: {:?}", chan_id, e),
                            }
                        } else if let Some(first_id) = chunk.first() {
                            if let Err(e) = state.http.delete_message(chan_id, *first_id).await {
                                warn!("Failed to delete spam message {} in {}: {:?}", first_id, chan_id, e);
                            }
                        }
                    }
                }
                action_log = "Deleted Messages".to_string();
            }

            // Determine action
            let action = if !antispam_cfg.action.is_empty() {
                antispam_cfg.action.as_str()
            } else if antispam_cfg.timeout {
                "timeout"
            } else {
                "none"
            };

            match action {
                "timeout" => {
                    apply_punishment(
                        &state.http,
                        guild_id,
                        author_id,
                        "timeout",
                        antispam_cfg.timeout_duration,
                        "Spamming messages",
                    )
                    .await;
                    action_log = if action_log == "None" { "Timeout".to_string() } else { format!("{} & Timeout", action_log) };
                }
                "kick" => {
                    apply_punishment(
                        &state.http,
                        guild_id,
                        author_id,
                        "kick",
                        0,
                        "Spamming messages",
                    )
                    .await;
                    action_log = if action_log == "None" { "Kick".to_string() } else { format!("{} & Kick", action_log) };
                }
                "ban" => {
                    apply_punishment(
                        &state.http,
                        guild_id,
                        author_id,
                        "ban",
                        0,
                        "Spamming messages",
                    )
                    .await;
                    action_log = if action_log == "None" { "Ban".to_string() } else { format!("{} & Ban", action_log) };
                }
                _ => {}
            }

            let log_embed = apply_embed_footer(
                EmbedBuilder::new()
                    .title("Spam Detection")
                    .color(0xFF0000)
                    .field(EmbedFieldBuilder::new("User", format!("<@{}>", author_id)).inline().build())
                    .field(EmbedFieldBuilder::new("Action", action_log).inline().build())
                    .field(EmbedFieldBuilder::new("Channel", format!("<#{}>", message.channel_id)).inline().build())
                    .field(EmbedFieldBuilder::new("Detected At", discord_timestamp_now()).build()),
                &embed_footer,
            )
            .build();

            send_log_embed(&state, log_embed).await;
        }
    }
}

/// Isolates a member who sends a message in the configured hack-trap channel.
/// Uses a single bulk role update request and bounded channel permission concurrency
/// to prevent triggering Discord's 50/sec global rate limit.
///
/// Inputs:
/// - `state`: Shared bot application state.
/// - `guild_id`: Target Discord Guild ID.
/// - `message`: The trigger message sent in the hack channel.
/// - `member_roles`: Slice of roles currently held by the member.
///
/// Outputs:
/// - None.
async fn handle_hack_channel_trigger(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    message: &Message,
    member_roles: &[Id<RoleMarker>],
) {
    let (hacked_role_id_str, hacked_view_channel_id_str, embed_footer) = {
        let cfg = state.config.read();
        (
            cfg.hack_mode.hacked_role_id.clone(),
            cfg.hack_mode.hacked_view_channel_id.clone(),
            cfg.embed_footer.clone(),
        )
    };

    let hacked_role_id = match hacked_role_id_str.parse::<u64>() {
        Ok(id) => Id::<RoleMarker>::new(id),
        Err(_) => return,
    };

    let hacked_view_channel_id = match hacked_view_channel_id_str.parse::<u64>() {
        Ok(id) => Id::<ChannelMarker>::new(id),
        Err(_) => return,
    };

    let author_id = message.author.id;

    // Delete trigger message first
    if let Err(e) = state.http.delete_message(message.channel_id, message.id).await {
        warn!("Failed to delete hack channel trigger message {}: {:?}", message.id, e);
    }

    // If member already has the hacked role, do not overwrite their saved roles or re-run channel overwrites
    if member_roles.contains(&hacked_role_id) {
        return;
    }

    // Save previous roles to DB (excluding hacked_role_id itself so original roles are never lost)
    let role_strings: Vec<String> = member_roles
        .iter()
        .filter(|&&r| r != hacked_role_id)
        .map(|r| r.get().to_string())
        .collect();

    let should_save = {
        let mut db = state.hacked_users.write();
        let key = author_id.get().to_string();
        match db.get(&key) {
            Some(existing) if !existing.roles.is_empty() && role_strings.is_empty() => false,
            _ => {
                db.insert(
                    key,
                    HackedUserEntry {
                        roles: role_strings,
                        guild_id: guild_id.get().to_string(),
                    },
                );
                true
            }
        }
    };
    if should_save {
        let _ = state.save_hacked_users().await;
    }

    // Replace all roles with `[hacked_role_id]` in a single atomic REST call (1 request instead of N+1)
    let target_roles = [hacked_role_id];
    let bulk_role_updated = match state
        .http
        .update_guild_member(guild_id, author_id)
        .roles(&target_roles)
        .await
    {
        Ok(_) => true,
        Err(e) => {
            warn!(
                "Bulk role replacement failed for user {} (falling back to individual role changes): {:?}",
                author_id, e
            );
            false
        }
    };

    if !bulk_role_updated {
        for role_id in member_roles {
            if *role_id != hacked_role_id {
                if let Err(e) = state.http.remove_guild_member_role(guild_id, author_id, *role_id).await {
                    warn!("Failed to remove role {} from {}: {:?}", role_id, author_id, e);
                }
            }
        }
        if let Err(e) = state.http.add_guild_member_role(guild_id, author_id, hacked_role_id).await {
            warn!("Failed to add hacked role {} to {}: {:?}", hacked_role_id, author_id, e);
        }
    }

    // Update permission overwrites
    // 1. Allow view on hacked view channel
    let allow_overwrite = PermissionOverwrite {
        allow: Some(Permissions::VIEW_CHANNEL),
        deny: Some(Permissions::SEND_MESSAGES),
        id: author_id.cast(),
        kind: PermissionOverwriteType::Member,
    };
    if let Err(e) = state
        .http
        .update_channel_permission(hacked_view_channel_id, &allow_overwrite)
        .await
    {
        warn!(
            "Failed to set view overwrite on channel {} for {}: {:?}",
            hacked_view_channel_id, author_id, e
        );
    }

    // 2. Deny view on other channels with bounded concurrency via Semaphore to prevent 429 global rate-limits
    if let Ok(res) = state.http.guild_channels(guild_id).await {
        if let Ok(channels) = res.model().await {
            let mut tasks = Vec::new();
            for ch in channels {
                if ch.id == hacked_view_channel_id {
                    continue;
                }

                // Skip channels that already deny VIEW_CHANNEL for this member
                let already_denied = ch
                    .permission_overwrites
                    .as_ref()
                    .map(|overwrites| {
                        overwrites.iter().any(|ow| {
                            ow.id.get() == author_id.get()
                                && ow.deny.contains(Permissions::VIEW_CHANNEL)
                        })
                    })
                    .unwrap_or(false);

                if already_denied {
                    continue;
                }

                let http = state.http.clone();
                let semaphore = state.channel_perm_semaphore.clone();
                let cid = ch.id;
                tasks.push(tokio::spawn(async move {
                    let _permit = semaphore.acquire_owned().await.ok();
                    let deny_overwrite = PermissionOverwrite {
                        allow: None,
                        deny: Some(Permissions::VIEW_CHANNEL),
                        id: author_id.cast(),
                        kind: PermissionOverwriteType::Member,
                    };
                    if let Err(e) = http.update_channel_permission(cid, &deny_overwrite).await {
                        warn!("Failed to update channel permission on {} for {}: {:?}", cid, author_id, e);
                    }
                }));
            }
            let _ = join_all(tasks).await;
        }
    }

    // Send log embed
    let log_embed = apply_embed_footer(
        EmbedBuilder::new()
            .title("Hack Channel Triggered")
            .color(0xFF0000)
            .field(EmbedFieldBuilder::new("User", format!("<@{}>", author_id)).inline().build())
            .field(EmbedFieldBuilder::new("Channel", format!("<#{}>", message.channel_id)).inline().build())
            .field(EmbedFieldBuilder::new("Hacked Role", format!("<@&{}>", hacked_role_id)).inline().build())
            .field(EmbedFieldBuilder::new("Detected At", discord_timestamp_now()).build()),
        &embed_footer,
    )
    .build();

    send_log_embed(state, log_embed).await;
}
