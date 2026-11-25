use twilight_model::application::interaction::application_command::CommandData;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::MessageFlags;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

use crate::state::AppState;
use crate::utils::{
    apply_embed_footer, discord_timestamp_now, is_owner_or_whitelisted, parse_webhook_url,
};

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
                content: Some("❌ غير مصرح لك! فقط مالك السيرفر أو الأعضاء في القائمة البيضاء يمكنهم تعيين ويب هوك السجلات.".to_string()),
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

    // Extract target url option
    let mut target_url = None;
    for option in &command_data.options {
        if option.name == "url" {
            if let twilight_model::application::interaction::application_command::CommandOptionValue::String(ref url) = option.value {
                target_url = Some(url.clone());
            }
        }
    }

    let webhook_url = match target_url {
        Some(url) => url,
        None => {
            let response = InteractionResponse {
                kind: InteractionResponseType::ChannelMessageWithSource,
                data: Some(twilight_model::http::interaction::InteractionResponseData {
                    content: Some("❌ يرجى إدخال رابط Webhook صحيح.".to_string()),
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
    };

    let (webhook_id, token) = match parse_webhook_url(&webhook_url) {
        Some(pair) => pair,
        None => {
            let response = InteractionResponse {
                kind: InteractionResponseType::ChannelMessageWithSource,
                data: Some(twilight_model::http::interaction::InteractionResponseData {
                    content: Some("❌ رابط Webhook غير صالح! يرجى التأكد من نسخ رابط الويب هوك بشكل كامل من إعدادات القناة في ديسكورد.".to_string()),
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
    };

    let embed_footer = {
        let cfg = state.config.read();
        cfg.embed_footer.clone()
    };

    // Test sending an embed via this webhook
    let test_embed = apply_embed_footer(
        EmbedBuilder::new()
            .color(0x0099FF)
            .title("🔔 اختبار سجلات الحماية عبر Webhook")
            .description("تم ربط وتعيين هذا الـ Webhook بنجاح كوجهة لسجلات وإشعارات الحماية.")
            .field(EmbedFieldBuilder::new("👤 المسؤول", format!("<@{}>", user_id)).build())
            .field(EmbedFieldBuilder::new("⏱️ الوقت", discord_timestamp_now()).build()),
        &embed_footer,
    )
    .build();

    match state.http.execute_webhook(webhook_id, &token).embeds(&[test_embed]) {
        Ok(req) => {
            if let Err(e) = req.await {
                let response = InteractionResponse {
                    kind: InteractionResponseType::ChannelMessageWithSource,
                    data: Some(twilight_model::http::interaction::InteractionResponseData {
                        content: Some(format!("❌ فشل إرسال رسالة تجريبية عبر الـ Webhook: {e}")),
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
        }
        Err(e) => {
            let response = InteractionResponse {
                kind: InteractionResponseType::ChannelMessageWithSource,
                data: Some(twilight_model::http::interaction::InteractionResponseData {
                    content: Some(format!("❌ تعذر تكوين رسالة الـ Webhook: {e}")),
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
    }

    // Update config
    {
        let mut cfg = state.config.write();
        cfg.log_webhook_url = webhook_url;
    }
    let _ = state.save_config().await;

    // Create confirmation embed
    let success_embed = apply_embed_footer(
        EmbedBuilder::new()
            .color(0x00FF00)
            .title("✅ تم تعيين Webhook السجلات بنجاح!")
            .description("تم تفعيل وتعيين الـ Webhook بنجاح لإرسال جميع تنبيهات وسجلات الحماية."),
        &embed_footer,
    )
    .build();

    let response = InteractionResponse {
        kind: InteractionResponseType::ChannelMessageWithSource,
        data: Some(twilight_model::http::interaction::InteractionResponseData {
            embeds: Some(vec![success_embed]),
            flags: Some(MessageFlags::EPHEMERAL),
            ..Default::default()
        }),
    };

    state
        .http
        .interaction(interaction.application_id)
        .create_response(interaction.id, &interaction.token, &response)
        .await?;

    Ok(())
}
