use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;
use crate::error::{BotError, BotResult};

/// Main configuration structure for the bot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(rename = "DISCORD_TOKEN", default)]
    pub discord_token: String,

    pub antispam: AntiSpamConfig,
    pub antilinks: AntiLinksConfig,
    pub channel_monitoring: ActivityConfig,
    pub role_monitoring: RoleMonitoringConfig,
    pub kick_monitoring: KickMonitoringConfig,
    pub ban_monitoring: BanMonitoringConfig,
    pub bot_monitoring: BotMonitoringConfig,
    pub role_edit_monitoring: EditMonitoringConfig,
    pub server_edit_monitoring: EditMonitoringConfig,
    pub hack_mode: HackModeConfig,

    pub allowed_guild_id: String,
    #[serde(default, alias = "logChannelId", alias = "webhook_url")]
    pub log_webhook_url: String,
    #[serde(default = "default_embed_footer")]
    pub embed_footer: String,
    pub whitelist_roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntiSpamConfig {
    pub active: bool,
    pub message_count: u64,
    pub time: u64,
    pub delete_messages: bool,
    pub timeout: bool,
    pub timeout_duration: u64,
    #[serde(default = "default_action")]
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntiLinksConfig {
    pub active: bool,
    pub delete_messages: bool,
    pub actions: AntiLinksActions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntiLinksActions {
    pub timeout: bool,
    pub timeout_duration: u64,
    pub kick: bool,
    pub ban: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityConfig {
    pub active: bool,
    pub max_channel_changes: u64,
    pub time_frame: u64,
    pub action: String,
    pub timeout_duration: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleMonitoringConfig {
    pub active: bool,
    pub max_role_changes: u64,
    pub time_frame: u64,
    pub action: String,
    pub timeout_duration: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KickMonitoringConfig {
    pub active: bool,
    pub max_kicks: u64,
    pub time_frame: u64,
    pub action: String,
    pub timeout_duration: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BanMonitoringConfig {
    pub active: bool,
    pub max_bans: u64,
    pub time_frame: u64,
    pub action: String,
    pub timeout_duration: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotMonitoringConfig {
    pub active: bool,
    pub action: String,
    pub bot_action: String,
    pub timeout_duration: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditMonitoringConfig {
    pub active: bool,
    pub max_edits: u64,
    pub time_frame: u64,
    pub action: String,
    pub timeout_duration: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HackModeConfig {
    pub active: bool,
    pub hack_channel_id: String,
    pub hacked_role_id: String,
    pub hacked_view_channel_id: String,
}

fn default_action() -> String {
    "kick".to_string()
}

fn default_embed_footer() -> String {
    "zrx".to_string()
}

/// Loads key-value pairs from a local `.env` file into the process environment if present.
///
/// Inputs:
/// - `dotenv_path`: Path to the `.env` file.
///
/// Outputs:
/// - None (populates `std::env` variables if they are not already set).
pub fn load_dotenv_if_present<P: AsRef<Path>>(dotenv_path: P) {
    let Ok(content) = fs::read_to_string(dotenv_path) else {
        return;
    };
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = trimmed.split_once('=') {
            let key = key.trim();
            let val = val.trim().trim_matches('"').trim_matches('\'');
            if !key.is_empty() && env::var_os(key).is_none() {
                env::set_var(key, val);
            }
        }
    }
}

impl Config {
    /// Resolves the Discord bot token from the `DISCORD_TOKEN` environment variable first,
    /// falling back to `config.json` without mutating the serializable configuration.
    ///
    /// Inputs:
    /// - `&self`: Reference to the loaded configuration.
    ///
    /// Outputs:
    /// - `Option<String>`: Non-empty Discord bot token if available.
    pub fn resolve_token(&self) -> Option<String> {
        if let Ok(env_token) = env::var("DISCORD_TOKEN") {
            let trimmed = env_token.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        let cfg_token = self.discord_token.trim();
        if !cfg_token.is_empty() {
            return Some(cfg_token.to_string());
        }
        None
    }

    /// Loads configuration from a given file path and validates that a Discord token
    /// is available via environment variables (`.env`) or `config.json`.
    ///
    /// Inputs:
    /// - `path`: File path to the JSON configuration file.
    ///
    /// Outputs:
    /// - `BotResult<Self>`: Parsed and validated `Config` instance or a `BotError`.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> BotResult<Self> {
        load_dotenv_if_present(".env");
        let content = fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;

        if config.resolve_token().is_none() {
            return Err(BotError::NotFound(
                "DISCORD_TOKEN is missing in both environment variables (.env) and config.json"
                    .to_string(),
            ));
        }

        Ok(config)
    }

    /// Saves the current configuration asynchronously to a file without blocking Tokio worker threads.
    /// Uses a temporary file rename when supported, with fallback for single-file Docker bind mounts.
    ///
    /// Inputs:
    /// - `path`: Target file path to write the JSON configuration to.
    ///
    /// Outputs:
    /// - `BotResult<()>`: `Ok(())` on success or `BotError` on serialization/IO failure.
    pub async fn save_to_file_async<P: AsRef<Path>>(&self, path: P) -> BotResult<()> {
        let path_ref = path.as_ref();
        let content = serde_json::to_string_pretty(self)?;
        let tmp_path = path_ref.with_extension("json.tmp");

        if tokio::fs::write(&tmp_path, &content).await.is_ok() {
            if tokio::fs::rename(&tmp_path, path_ref).await.is_ok() {
                return Ok(());
            }
            let _ = tokio::fs::remove_file(&tmp_path).await;
        }

        tokio::fs::write(path_ref, content).await?;
        Ok(())
    }
}
