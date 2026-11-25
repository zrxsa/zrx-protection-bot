use futures_util::future::join_all;
use tracing::warn;
use twilight_model::application::interaction::application_command::CommandData;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::MessageFlags;
use twilight_model::channel::permission_overwrite::PermissionOverwriteType;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_model::id::marker::RoleMarker;
use twilight_model::id::Id;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

use crate::state::AppState;
use crate::utils::{
    apply_embed_footer, discord_timestamp_now, is_owner_or_whitelisted, send_log_embed,
};

/// Executes the `/unhack` command to remove the hacked role, restore saved roles in a single
/// bulk request, and clean up channel permission overwrites with bounded concurrency.
///
/// Inputs:
/// - `state`: Shared bot application state.
/// - `interaction`: Discord slash command interaction payload.
/// - `command_data`: Parsed application command options.
///
/// Outputs:
/// - `anyhow::Result<()>`: `Ok(())` on completion or an error if interaction response fails.
pub async fn execute(
    state: &AppState,
    interaction: &Interaction,
    command_data: &CommandData,
) -> anyhow::Result<()> {
    let guild_id = match interaction.guild_id {
        Some(gid) => gid,
        None => return Ok(()),
    };

    let user_id = match interaction.author_id() {
        Some(uid) => uid,
        None => return Ok(()),
    };

    let member_roles = interaction
        .member
        .as_ref()
        .map(|m| m.roles.as_slice());

    if !is_owner_or_whitelisted(state, guild_id, user_id, member_roles).await {
        let response = InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(twilight_model::http::interaction::InteractionResponseData {
                content: Some("❌ غير مصرح لك! فقط مالك السيرفر أو الأعضاء في القائمة البيضاء يمكنهم استخدام هذا الأمر.".to_string()),
                flags: Some(MessageFlags::EPHEMERAL),
                ..Default::default()
            }),
        };
        state
            .http
            .interaction(interaction.application_id)
            .create_response(interaction.id, &interaction.token, &response)
            .await?;
        return Ok(());
    }

    // Acknowledge interaction (defer)
    let defer = InteractionResponse {
        kind: InteractionResponseType::DeferredChannelMessageWithSource,
        data: Some(twilight_model::http::interaction::InteractionResponseData {
            flags: Some(MessageFlags::EPHEMERAL),
            ..Default::default()
        }),
    };
    state
        .http
        .interaction(interaction.application_id)
        .create_response(interaction.id, &interaction.token, &defer)
        .await?;

    // Extract target user
    let mut target_user_id = None;
    for option in &command_data.options {
        if option.name == "user" {
            if let twilight_model::application::interaction::application_command::CommandOptionValue::User(uid) = option.value {
                target_user_id = Some(uid);
            }
        }
    }

    let target_uid = match target_user_id {
        Some(uid) => uid,
        None => {
            if let Ok(req) = state
                .http
                .interaction(interaction.application_id)
                .update_response(&interaction.token)
                .content(Some("❌ يرجى تحديد العضو."))
            {
                let _ = req.await;
            }
            return Ok(());
        }
    };

    let hacked_role_id_str = {
        let cfg = state.config.read();
        cfg.hack_mode.hacked_role_id.clone()
    };

    let hacked_role_id = match hacked_role_id_str.parse::<u64>() {
        Ok(id) => Id::<RoleMarker>::new(id),
        Err(_) => {
            if let Ok(req) = state
                .http
                .interaction(interaction.application_id)
                .update_response(&interaction.token)
                .content(Some("❌ لم يتم ضبط دور المخترق في الإعدادات."))
            {
                let _ = req.await;
            }
            return Ok(());
        }
    };

    // Fetch target member
    let target_member = match state.http.guild_member(guild_id, target_uid).await {
        Ok(res) => match res.model().await {
            Ok(m) => m,
            Err(_) => {
                if let Ok(req) = state
                    .http
                    .interaction(interaction.application_id)
                    .update_response(&interaction.token)
                    .content(Some("❌ لم يتم العثور على العضو في السيرفر."))
                {
                    let _ = req.await;
                }
                return Ok(());
            }
        },
        Err(_) => {
            if let Ok(req) = state
                .http
                .interaction(interaction.application_id)
                .update_response(&interaction.token)
                .content(Some("❌ لم يتم العثور على العضو في السيرفر."))
            {
                let _ = req.await;
            }
            return Ok(());
        }
    };

    if !target_member.roles.contains(&hacked_role_id) {
        if let Ok(req) = state
            .http
            .interaction(interaction.application_id)
            .update_response(&interaction.token)
            .content(Some(&format!("❌ العضو <@{}> ليس لديه دور المخترق.", target_uid)))
        {
            let _ = req.await;
        }
        return Ok(());
    }

    // Retrieve saved roles from DB
    let target_key = target_uid.get().to_string();
    let saved_entry = {
        let mut db = state.hacked_users.write();
        db.remove(&target_key)
    };

    let mut restored_roles: Vec<Id<RoleMarker>> = target_member
        .roles
        .iter()
        .copied()
        .filter(|&r| r != hacked_role_id)
        .collect();

    if let Some(ref entry) = saved_entry {
        let _ = state.save_hacked_users().await;

        for role_id_str in &entry.roles {
            if let Ok(rid) = role_id_str.parse::<u64>() {
                let role_id = Id::<RoleMarker>::new(rid);
                if role_id != hacked_role_id && !restored_roles.contains(&role_id) {
                    restored_roles.push(role_id);
                }
            }
        }
    }

    // Replace member roles atomically in a single HTTP request (fallback to individual calls if needed)
    let bulk_restored = match state
        .http
        .update_guild_member(guild_id, target_uid)
        .roles(&restored_roles)
        .await
    {
        Ok(_) => true,
        Err(e) => {
            warn!(
                "Bulk role restoration failed for {} (falling back to individual role updates): {:?}",
                target_uid, e
            );
            false
        }
    };

    if !bulk_restored {
        if let Err(e) = state
            .http
            .remove_guild_member_role(guild_id, target_uid, hacked_role_id)
            .await
        {
            warn!("Failed to remove hacked role from {}: {:?}", target_uid, e);
        }

        for role_id in &restored_roles {
            if let Err(e) = state
                .http
                .add_guild_member_role(guild_id, target_uid, *role_id)
                .await
            {
                warn!("Failed to restore role {} to {}: {:?}", role_id, target_uid, e);
            }
        }
    }

    // Clear channel overwrites with bounded concurrency via Semaphore
    if let Ok(res) = state.http.guild_channels(guild_id).await {
        if let Ok(channels) = res.model().await {
            let mut tasks = Vec::new();
            for channel in channels {
                if let Some(ref overwrites) = channel.permission_overwrites {
                    for overwrite in overwrites {
                        if overwrite.kind == PermissionOverwriteType::Member && overwrite.id.get() == target_uid.get() {
                            let http_clone = state.http.clone();
                            let semaphore = state.channel_perm_semaphore.clone();
                            let cid = channel.id;
                            tasks.push(tokio::spawn(async move {
                                let _permit = semaphore.acquire_owned().await.ok();
                                if let Err(e) = http_clone
                                    .delete_channel_permission(cid)
                                    .member(target_uid)
                                    .await
                                {
                                    warn!("Failed to delete channel permission on {} for {}: {:?}", cid, target_uid, e);
                                }
                            }));
                        }
                    }
                }
            }
            let _ = join_all(tasks).await;
        }
    }

    let embed_footer = {
        let cfg = state.config.read();
        cfg.embed_footer.clone()
    };

    let success_embed = apply_embed_footer(
        EmbedBuilder::new()
            .color(0x00FF00)
            .title("✅ تم فك الاختراق بنجاح")
            .description(format!(
                "تم إزالة دور المخترق من <@{}> وإعادة أدواره السابقة وصلاحية رؤية القنوات.",
                target_uid
            ))
            .field(EmbedFieldBuilder::new("👤 العضو", format!("<@{}>", target_uid)).inline().build())
            .field(EmbedFieldBuilder::new("🛠️ المسؤول", format!("<@{}>", user_id)).inline().build())
            .field(EmbedFieldBuilder::new("⏱️ الوقت", discord_timestamp_now()).build()),
        &embed_footer,
    )
    .build();

    if let Ok(req) = state
        .http
        .interaction(interaction.application_id)
        .update_response(&interaction.token)
        .embeds(Some(&[success_embed]))
    {
        let _ = req.await;
    }

    let log_embed = apply_embed_footer(
        EmbedBuilder::new()
            .title("Unhack Command Used")
            .color(0xFF0000)
            .field(EmbedFieldBuilder::new("User", format!("<@{}>", target_uid)).inline().build())
            .field(EmbedFieldBuilder::new("Admin", format!("<@{}>", user_id)).inline().build())
            .field(EmbedFieldBuilder::new("Detected At", discord_timestamp_now()).build()),
        &embed_footer,
    )
    .build();

    send_log_embed(state, log_embed).await;

    Ok(())
}
