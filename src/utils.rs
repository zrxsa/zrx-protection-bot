use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info, warn};
use twilight_http::Client as HttpClient;
use twilight_model::channel::message::Embed;
use twilight_model::id::marker::{GuildMarker, RoleMarker, UserMarker, WebhookMarker};
use twilight_model::id::Id;
use twilight_model::util::Timestamp;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder};

use crate::state::AppState;

/// Checks whether a guild is permitted by `allowed_guild_id`.
/// An empty `allowed_guild_id` permits any guild.
///
/// Inputs:
/// - `allowed_guild_id`: Configured guild ID string.
/// - `guild_id`: Incoming Discord Guild ID.
///
/// Outputs:
/// - `bool`: `true` if allowed, `false` otherwise.
pub fn is_allowed_guild(allowed_guild_id: &str, guild_id: Id<GuildMarker>) -> bool {
    let trimmed = allowed_guild_id.trim();
    if trimmed.is_empty() {
        return true;
    }
    trimmed
        .parse::<u64>()
        .map(|allowed_id| guild_id.get() == allowed_id)
        .unwrap_or(false)
}

/// Applies a configurable embed footer when `footer_text` is non-empty.
///
/// Inputs:
/// - `builder`: Embed builder being constructed.
/// - `footer_text`: Footer text from `config.json`.
///
/// Outputs:
/// - `EmbedBuilder`: Builder with footer attached if non-empty.
pub fn apply_embed_footer(builder: EmbedBuilder, footer_text: &str) -> EmbedBuilder {
    let trimmed = footer_text.trim();
    if trimmed.is_empty() {
        builder
    } else {
        builder.footer(EmbedFooterBuilder::new(trimmed))
    }
}

/// Check if a user is the guild owner or possesses any whitelisted role.
/// Uses cached `guild_owner_id` on `AppState` to avoid unnecessary REST calls.
///
/// Inputs:
/// - `state`: Shared bot application state.
/// - `guild_id`: Target Discord Guild ID.
/// - `user_id`: Target Discord User ID.
/// - `member_roles`: Optional slice of role IDs already known for the member.
///
/// Outputs:
/// - `bool`: `true` if the user is the guild owner or has a whitelisted role, `false` otherwise.
pub async fn is_owner_or_whitelisted(
    state: &AppState,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
    member_roles: Option<&[Id<RoleMarker>]>,
) -> bool {
    let (allowed_guild, whitelist_roles) = {
        let cfg = state.config.read();
        (cfg.allowed_guild_id.clone(), cfg.whitelist_roles.clone())
    };

    if !is_allowed_guild(&allowed_guild, guild_id) {
        return false;
    }

    // 1. Fast path: check cached guild owner ID
    if let Some(owner_id) = state.get_guild_owner_id() {
        if owner_id == user_id {
            return true;
        }
    }

    // 2. Fast path: if roles are already provided on the event, check whitelist before making HTTP calls
    if let Some(roles) = member_roles {
        for role in roles {
            let role_str = role.get().to_string();
            if whitelist_roles.contains(&role_str) {
                return true;
            }
        }
    }

    // 3. Populate guild owner cache if not yet known
    if state.get_guild_owner_id().is_none() {
        match state.http.guild(guild_id).await {
            Ok(res) => match res.model().await {
                Ok(guild) => {
                    state.set_guild_owner_id(guild.owner_id);
                    if guild.owner_id == user_id {
                        return true;
                    }
                }
                Err(e) => warn!("Failed to deserialize guild {} model: {:?}", guild_id, e),
            },
            Err(e) => warn!("Failed to fetch guild {} for owner check: {:?}", guild_id, e),
        }
    }

    // 4. Fetch member roles over HTTP only when not provided by caller
    if member_roles.is_none() {
        match state.http.guild_member(guild_id, user_id).await {
            Ok(res) => match res.model().await {
                Ok(member) => {
                    for role in &member.roles {
                        let role_str = role.get().to_string();
                        if whitelist_roles.contains(&role_str) {
                            return true;
                        }
                    }
                }
                Err(e) => warn!("Failed to deserialize member {} in guild {}: {:?}", user_id, guild_id, e),
            },
            Err(e) => warn!("Failed to fetch member {} in guild {}: {:?}", user_id, guild_id, e),
        }
    }

    false
}

/// Apply punitive actions (kick, ban, timeout) against a target member.
pub async fn apply_punishment(
    http: &HttpClient,
    guild_id: Id<GuildMarker>,
    user_id: Id<UserMarker>,
    action: &str,
    timeout_duration_ms: u64,
    reason: &str,
) {
    match action {
        "kick" => {
            if let Err(e) = http.remove_guild_member(guild_id, user_id).await {
                error!("Failed to kick user {}: {:?}", user_id, e);
            } else {
                info!("Successfully kicked user {} with reason '{}'", user_id, reason);
            }
        }
        "ban" => {
            if let Err(e) = http.create_ban(guild_id, user_id).await {
                error!("Failed to ban user {}: {:?}", user_id, e);
            } else {
                info!("Successfully banned user {} with reason '{}'", user_id, reason);
            }
        }
        "timeout" => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_micros();
            let timeout_micros = (timeout_duration_ms as u128) * 1000;
            let end_micros = (now + timeout_micros) as i64;

            if let Ok(ts) = Timestamp::from_micros(end_micros) {
                if let Ok(req) = http
                    .update_guild_member(guild_id, user_id)
                    .communication_disabled_until(Some(ts))
                {
                    if let Err(e) = req.await {
                        error!("Failed to timeout user {}: {:?}", user_id, e);
                    } else {
                        info!("Successfully applied timeout to user {}", user_id);
                    }
                }
            }
        }
        _ => {
            warn!("Unknown or 'none' punishment action: {}", action);
        }
    }
}

/// Parse a Discord webhook URL into its ID and token.
pub fn parse_webhook_url(url: &str) -> Option<(Id<WebhookMarker>, String)> {
    let clean = url.trim().trim_end_matches('/');
    let parts: Vec<&str> = clean.split('/').collect();
    if parts.len() >= 2 {
        let token = (*parts.last()?).to_string();
        let id_str = *parts.get(parts.len() - 2)?;
        let id_num: u64 = id_str.parse().ok()?;
        Some((Id::new(id_num), token))
    } else {
        None
    }
}

/// Send an embed to the configured log webhook.
pub async fn send_log_embed(state: &AppState, embed: Embed) {
    let webhook_url = {
        let cfg = state.config.read();
        cfg.log_webhook_url.clone()
    };

    if let Some((webhook_id, token)) = parse_webhook_url(&webhook_url) {
        if let Ok(req) = state.http.execute_webhook(webhook_id, &token).embeds(&[embed]) {
            if let Err(e) = req.await {
                error!("Failed to send log embed to webhook {}: {:?}", webhook_id, e);
            }
        }
    }
}

/// Helper function to get current unix timestamp in seconds.
pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Helper function to get current unix timestamp in milliseconds.
///
/// Inputs:
/// - None.
///
/// Outputs:
/// - `u64`: Current Unix epoch time in milliseconds.
pub fn now_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Format a discord timestamp string `<t:SECONDS:F>`.
pub fn discord_timestamp_now() -> String {
    format!("<t:{}:F>", now_unix_secs())
}
