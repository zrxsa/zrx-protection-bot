pub mod audit;
pub mod interaction;
pub mod message;

use std::sync::Arc;
use tracing::{error, info};
use twilight_gateway::Event;
use twilight_model::guild::audit_log::AuditLogEventType;

use crate::state::AppState;
use crate::utils::is_allowed_guild;

pub async fn handle_event(state: Arc<AppState>, event: Event) {
    match event {
        Event::Ready(ready) => {
            info!("Logged in as {}", ready.user.name);
            info!("Protection bot is now ready to use!");
            info!("-----------------------------------");

            state.set_bot_user_id(ready.user.id);

            let state_clone = state.clone();
            let app_id = ready.application.id;
            tokio::spawn(async move {
                if let Err(e) = crate::commands::register_commands(&state_clone, app_id).await {
                    error!("Failed to register slash commands: {:?}", e);
                }
            });
        }
        Event::GuildCreate(guild) => {
            let allowed_guild = {
                let cfg = state.config.read();
                cfg.allowed_guild_id.clone()
            };
            if is_allowed_guild(&allowed_guild, guild.id) {
                state.set_guild_owner_id(guild.owner_id);
            }
        }
        Event::MessageCreate(msg) => {
            let state_clone = state.clone();
            tokio::spawn(async move {
                message::handle_message(state_clone, msg.0).await;
            });
        }
        Event::InteractionCreate(interaction) => {
            let state_clone = state.clone();
            tokio::spawn(async move {
                interaction::handle_interaction(state_clone, interaction.0).await;
            });
        }
        Event::BanAdd(ban) => {
            let state_clone = state.clone();
            let guild_id = ban.guild_id;
            let target_id = Some(ban.user.id.get());
            tokio::spawn(async move {
                audit::handle_ban_activity(state_clone, guild_id, target_id).await;
            });
        }
        Event::MemberRemove(member) => {
            let state_clone = state.clone();
            let guild_id = member.guild_id;
            let target_id = Some(member.user.id.get());
            tokio::spawn(async move {
                audit::handle_kick_activity(state_clone, guild_id, target_id).await;
            });
        }
        Event::MemberAdd(member) => {
            if member.user.bot {
                let state_clone = state.clone();
                let guild_id = member.guild_id;
                let bot_id = member.user.id;
                tokio::spawn(async move {
                    audit::handle_bot_addition(state_clone, guild_id, bot_id).await;
                });
            }
        }
        Event::RoleCreate(role) => {
            let state_clone = state.clone();
            let guild_id = role.guild_id;
            let target_role_id = Some(role.role.id.get());
            tokio::spawn(async move {
                audit::handle_role_activity(
                    state_clone,
                    guild_id,
                    "Create",
                    AuditLogEventType::RoleCreate,
                    target_role_id,
                )
                .await;
            });
        }
        Event::RoleDelete(role) => {
            let state_clone = state.clone();
            let guild_id = role.guild_id;
            let target_role_id = Some(role.role_id.get());
            tokio::spawn(async move {
                audit::handle_role_activity(
                    state_clone,
                    guild_id,
                    "Delete",
                    AuditLogEventType::RoleDelete,
                    target_role_id,
                )
                .await;
            });
        }
        Event::RoleUpdate(role) => {
            let state_clone = state.clone();
            let guild_id = role.guild_id;
            let target_role_id = Some(role.role.id.get());
            tokio::spawn(async move {
                audit::handle_role_edit_activity(
                    state_clone,
                    guild_id,
                    AuditLogEventType::RoleUpdate,
                    target_role_id,
                )
                .await;
            });
        }
        Event::MemberUpdate(member) => {
            let state_clone = state.clone();
            let guild_id = member.guild_id;
            let target_user_id = Some(member.user.id.get());
            tokio::spawn(async move {
                audit::handle_role_edit_activity(
                    state_clone,
                    guild_id,
                    AuditLogEventType::MemberRoleUpdate,
                    target_user_id,
                )
                .await;
            });
        }
        Event::ChannelCreate(channel) => {
            if let Some(guild_id) = channel.guild_id {
                let state_clone = state.clone();
                let target_channel_id = Some(channel.id.get());
                tokio::spawn(async move {
                    audit::handle_channel_activity(
                        state_clone,
                        guild_id,
                        "Create",
                        AuditLogEventType::ChannelCreate,
                        target_channel_id,
                    )
                    .await;
                });
            }
        }
        Event::ChannelDelete(channel) => {
            if let Some(guild_id) = channel.guild_id {
                let state_clone = state.clone();
                let target_channel_id = Some(channel.id.get());
                tokio::spawn(async move {
                    audit::handle_channel_activity(
                        state_clone,
                        guild_id,
                        "Delete",
                        AuditLogEventType::ChannelDelete,
                        target_channel_id,
                    )
                    .await;
                });
            }
        }
        Event::GuildUpdate(guild) => {
            let allowed_guild = {
                let cfg = state.config.read();
                cfg.allowed_guild_id.clone()
            };
            if is_allowed_guild(&allowed_guild, guild.id) {
                state.set_guild_owner_id(guild.owner_id);
            }
            let state_clone = state.clone();
            let guild_id = guild.id;
            tokio::spawn(async move {
                audit::handle_server_edit_activity(state_clone, guild_id).await;
            });
        }
        _ => {}
    }
}
