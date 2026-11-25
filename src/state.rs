use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tracing::error;
use twilight_http::Client as HttpClient;
use twilight_model::id::marker::{ChannelMarker, MessageMarker, UserMarker};
use twilight_model::id::Id;

use crate::config::Config;
use crate::error::BotResult;

/// Maximum concurrent channel permission requests to avoid hitting Discord's 50/sec global rate limit.
const MAX_CONCURRENT_CHANNEL_PERM_OPS: usize = 4;

/// Maximum retention window for deduplicating processed audit log entry IDs.
const AUDIT_ENTRY_CACHE_TTL: Duration = Duration::from_secs(120);

/// Maximum idle retention for user rate-limit buckets before eviction during periodic cleanup.
const USER_CACHE_MAX_IDLE: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HackedUserEntry {
    pub roles: Vec<String>,
    #[serde(rename = "guildId")]
    pub guild_id: String,
}

pub type HackedUsersMap = HashMap<String, HackedUserEntry>;

/// Represents a cached message entry `(channel_id, message_id, timestamp)` for anti-spam tracking.
pub type SpamMessageRecord = (Id<ChannelMarker>, Id<MessageMarker>, Instant);

/// Global shared state across asynchronous worker tasks.
pub struct AppState {
    pub http: Arc<HttpClient>,
    pub config: Arc<RwLock<Config>>,
    pub config_path: String,
    pub hacked_users: Arc<RwLock<HackedUsersMap>>,
    pub hacked_users_path: String,

    // Caches using DashMap for high-throughput, lockless rate-limiting
    pub spam_cache: DashMap<Id<UserMarker>, Vec<SpamMessageRecord>>,
    pub ban_cache: DashMap<Id<UserMarker>, Vec<Instant>>,
    pub kick_cache: DashMap<Id<UserMarker>, Vec<Instant>>,
    pub role_cache: DashMap<Id<UserMarker>, Vec<Instant>>,
    pub role_edit_cache: DashMap<Id<UserMarker>, Vec<Instant>>,
    pub channel_cache: DashMap<Id<UserMarker>, Vec<Instant>>,
    pub server_edit_cache: DashMap<Id<UserMarker>, Vec<Instant>>,

    /// Deduplication cache of already-processed AuditLogEntry IDs mapped to when they were seen.
    pub processed_audit_entries: DashMap<u64, Instant>,

    /// Semaphore bounding concurrent channel permission overwrite requests.
    pub channel_perm_semaphore: Arc<Semaphore>,

    pub bot_user_id: AtomicU64,
    pub guild_owner_id: AtomicU64,
}

impl AppState {
    /// Creates a new `AppState` instance and loads `hacked_users.json` if it exists.
    ///
    /// Inputs:
    /// - `http`: Shared Twilight HTTP client.
    /// - `config`: Initial loaded bot configuration.
    /// - `config_path`: File path to `config.json`.
    /// - `hacked_users_path`: File path to `hacked_users.json`.
    ///
    /// Outputs:
    /// - `Self`: Initialized `AppState`.
    pub fn new(
        http: Arc<HttpClient>,
        config: Config,
        config_path: String,
        hacked_users_path: String,
    ) -> Self {
        let hacked_users: HackedUsersMap = if Path::new(&hacked_users_path).exists() {
            match fs::read_to_string(&hacked_users_path) {
                Ok(data) => serde_json::from_str(&data).unwrap_or_else(|e| {
                    error!("Failed to parse {}: {:?}", hacked_users_path, e);
                    HashMap::new()
                }),
                Err(e) => {
                    error!("Failed to read {}: {:?}", hacked_users_path, e);
                    HashMap::new()
                }
            }
        } else {
            HashMap::new()
        };

        Self {
            http,
            config: Arc::new(RwLock::new(config)),
            config_path,
            hacked_users: Arc::new(RwLock::new(hacked_users)),
            hacked_users_path,
            spam_cache: DashMap::new(),
            ban_cache: DashMap::new(),
            kick_cache: DashMap::new(),
            role_cache: DashMap::new(),
            role_edit_cache: DashMap::new(),
            channel_cache: DashMap::new(),
            server_edit_cache: DashMap::new(),
            processed_audit_entries: DashMap::new(),
            channel_perm_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_CHANNEL_PERM_OPS)),
            bot_user_id: AtomicU64::new(0),
            guild_owner_id: AtomicU64::new(0),
        }
    }

    /// Set current bot user ID.
    pub fn set_bot_user_id(&self, id: Id<UserMarker>) {
        self.bot_user_id.store(id.get(), Ordering::SeqCst);
    }

    /// Get current bot user ID if known.
    pub fn get_bot_user_id(&self) -> Option<Id<UserMarker>> {
        let id = self.bot_user_id.load(Ordering::SeqCst);
        if id != 0 {
            Some(Id::new(id))
        } else {
            None
        }
    }

    /// Caches the guild owner's user ID in memory to avoid repeated HTTP guild lookups.
    ///
    /// Inputs:
    /// - `id`: Discord User ID of the guild owner.
    ///
    /// Outputs:
    /// - None.
    pub fn set_guild_owner_id(&self, id: Id<UserMarker>) {
        self.guild_owner_id.store(id.get(), Ordering::SeqCst);
    }

    /// Retrieves the cached guild owner's user ID if available.
    ///
    /// Inputs:
    /// - `&self`: Shared reference to `AppState`.
    ///
    /// Outputs:
    /// - `Option<Id<UserMarker>>`: `Some(owner_id)` if cached, `None` otherwise.
    pub fn get_guild_owner_id(&self) -> Option<Id<UserMarker>> {
        let id = self.guild_owner_id.load(Ordering::SeqCst);
        if id != 0 {
            Some(Id::new(id))
        } else {
            None
        }
    }

    /// Checks whether an audit log entry ID has already been processed, and marks it as processed.
    ///
    /// Inputs:
    /// - `entry_id`: Raw `u64` Discord Snowflake ID of the `AuditLogEntry`.
    ///
    /// Outputs:
    /// - `bool`: `true` if the entry is new and was just marked; `false` if it was already processed.
    pub fn mark_audit_entry_if_new(&self, entry_id: u64) -> bool {
        let now = Instant::now();
        if self.processed_audit_entries.contains_key(&entry_id) {
            return false;
        }
        self.processed_audit_entries.insert(entry_id, now).is_none()
    }

    /// Evicts expired entries and empty buckets from all in-memory `DashMap` caches to prevent memory leaks.
    ///
    /// Inputs:
    /// - `&self`: Shared reference to `AppState`.
    ///
    /// Outputs:
    /// - None.
    pub fn cleanup_expired_caches(&self) {
        let now = Instant::now();

        self.processed_audit_entries
            .retain(|_, seen_at| now.duration_since(*seen_at) < AUDIT_ENTRY_CACHE_TTL);

        self.spam_cache.retain(|_, entries| {
            entries.retain(|(_, _, t)| now.duration_since(*t) < USER_CACHE_MAX_IDLE);
            !entries.is_empty()
        });

        let clean_instant_map = |map: &DashMap<Id<UserMarker>, Vec<Instant>>| {
            map.retain(|_, entries| {
                entries.retain(|t| now.duration_since(*t) < USER_CACHE_MAX_IDLE);
                !entries.is_empty()
            });
        };

        clean_instant_map(&self.ban_cache);
        clean_instant_map(&self.kick_cache);
        clean_instant_map(&self.role_cache);
        clean_instant_map(&self.role_edit_cache);
        clean_instant_map(&self.channel_cache);
        clean_instant_map(&self.server_edit_cache);
    }

    /// Saves updated config to disk asynchronously and logs any failure.
    ///
    /// Inputs:
    /// - `&self`: Shared reference to `AppState`.
    ///
    /// Outputs:
    /// - `BotResult<()>`: `Ok(())` on success or `BotError` on failure.
    pub async fn save_config(&self) -> BotResult<()> {
        let config = self.config.read().clone();
        if let Err(e) = config.save_to_file_async(&self.config_path).await {
            error!("Failed to save configuration to {}: {:?}", self.config_path, e);
            return Err(e);
        }
        Ok(())
    }

    /// Saves hacked users map to disk asynchronously and logs any failure.
    ///
    /// Inputs:
    /// - `&self`: Shared reference to `AppState`.
    ///
    /// Outputs:
    /// - `BotResult<()>`: `Ok(())` on success or `BotError` on failure.
    pub async fn save_hacked_users(&self) -> BotResult<()> {
        let content = {
            let data = self.hacked_users.read();
            serde_json::to_string_pretty(&*data)?
        };
        let path_ref = Path::new(&self.hacked_users_path);
        let tmp_path = path_ref.with_extension("json.tmp");

        if tokio::fs::write(&tmp_path, &content).await.is_ok() {
            if tokio::fs::rename(&tmp_path, path_ref).await.is_ok() {
                return Ok(());
            }
            let _ = tokio::fs::remove_file(&tmp_path).await;
        }

        if let Err(e) = tokio::fs::write(path_ref, content).await {
            error!(
                "Failed to save hacked users database to {}: {:?}",
                self.hacked_users_path, e
            );
            return Err(e.into());
        }
        Ok(())
    }
}
