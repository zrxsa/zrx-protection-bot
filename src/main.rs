mod commands;
mod config;
mod error;
mod events;
mod state;
mod utils;

use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;
use twilight_gateway::{Intents, Shard, ShardId};
use twilight_http::Client as HttpClient;

use crate::config::Config;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Starting high-performance Discord protection bot...");

    let config_path = "config.json".to_string();
    let hacked_users_path = "hacked_users.json".to_string();

    let config = match Config::load_from_file(&config_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Failed to load {}: {:?}", config_path, e);
            return Err(e.into());
        }
    };

    let token = config.resolve_token().ok_or_else(|| {
        anyhow::anyhow!(
            "DISCORD_TOKEN is missing in both environment variables (.env) and config.json"
        )
    })?;
    let http = Arc::new(HttpClient::new(token.clone()));

    let state = Arc::new(AppState::new(
        http.clone(),
        config,
        config_path,
        hacked_users_path,
    ));

    // Spawn periodic cache cleanup task with an explicit shutdown channel to prevent memory leaks
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let cleanup_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    cleanup_state.cleanup_expired_caches();
                }
                _ = shutdown_rx.changed() => {
                    break;
                }
            }
        }
    });

    let intents = Intents::GUILDS
        | Intents::GUILD_MEMBERS
        | Intents::GUILD_MESSAGES
        | Intents::MESSAGE_CONTENT
        | Intents::GUILD_MODERATION;

    let mut shard = Shard::new(ShardId::ONE, token, intents);

    info!("Connecting to Discord Gateway...");

    loop {
        let event = match shard.next_event().await {
            Ok(event) => event,
            Err(source) => {
                error!("Error receiving gateway event: {:?}", source);
                if source.is_fatal() {
                    break;
                }
                continue;
            }
        };

        events::handle_event(state.clone(), event).await;
    }

    let _ = shutdown_tx.send(true);
    Ok(())
}
