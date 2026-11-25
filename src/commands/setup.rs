use twilight_model::application::interaction::application_command::CommandData;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::channel::message::MessageFlags;
use twilight_model::channel::message::ReactionType;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

use crate::config::Config;
use crate::state::AppState;
use crate::utils::{apply_embed_footer, is_owner_or_whitelisted};

pub fn create_main_setup_components(config: &Config) -> Vec<Component> {
    let btn_style = |active: bool| if active { ButtonStyle::Success } else { ButtonStyle::Danger };

    let r1 = Component::ActionRow(ActionRow {
        components: vec![
            Component::Button(Button {
                custom_id: Some("view_antispam".to_string()),
                disabled: false,
                emoji: Some(ReactionType::Unicode { name: "🛡️".to_string() }),
                label: Some("مكافحة السبام".to_string()),
                style: btn_style(config.antispam.active),
                url: None,
            }),
            Component::Button(Button {
                custom_id: Some("view_antilinks".to_string()),
                disabled: false,
                emoji: Some(ReactionType::Unicode { name: "🔗".to_string() }),
                label: Some("مكافحة الروابط".to_string()),
                style: btn_style(config.antilinks.active),
                url: None,
            }),
            Component::Button(Button {
                custom_id: Some("view_channelMonitoring".to_string()),
                disabled: false,
                emoji: Some(ReactionType::Unicode { name: "📝".to_string() }),
                label: Some("حماية القنوات".to_string()),
                style: btn_style(config.channel_monitoring.active),
                url: None,
            }),
        ],
    });

    let r2 = Component::ActionRow(ActionRow {
        components: vec![
            Component::Button(Button {
                custom_id: Some("view_roleMonitoring".to_string()),
                disabled: false,
                emoji: Some(ReactionType::Unicode { name: "👑".to_string() }),
                label: Some("حماية الأدوار".to_string()),
                style: btn_style(config.role_monitoring.active),
                url: None,
            }),
            Component::Button(Button {
                custom_id: Some("view_kickMonitoring".to_string()),
                disabled: false,
                emoji: Some(ReactionType::Unicode { name: "🚫".to_string() }),
                label: Some("مراقبة الطرد".to_string()),
                style: btn_style(config.kick_monitoring.active),
                url: None,
            }),
            Component::Button(Button {
                custom_id: Some("view_banMonitoring".to_string()),
                disabled: false,
                emoji: Some(ReactionType::Unicode { name: "⛔".to_string() }),
                label: Some("مراقبة الحظر".to_string()),
                style: btn_style(config.ban_monitoring.active),
                url: None,
            }),
        ],
    });

    let r3 = Component::ActionRow(ActionRow {
        components: vec![
            Component::Button(Button {
                custom_id: Some("view_botMonitoring".to_string()),
                disabled: false,
                emoji: Some(ReactionType::Unicode { name: "🤖".to_string() }),
                label: Some("حماية البوتات".to_string()),
                style: btn_style(config.bot_monitoring.active),
                url: None,
            }),
            Component::Button(Button {
                custom_id: Some("view_roleEditMonitoring".to_string()),
                disabled: false,
                emoji: Some(ReactionType::Unicode { name: "✏️".to_string() }),
                label: Some("مراقبة تعديل الأدوار".to_string()),
                style: btn_style(config.role_edit_monitoring.active),
                url: None,
            }),
            Component::Button(Button {
                custom_id: Some("view_serverEditMonitoring".to_string()),
                disabled: false,
                emoji: Some(ReactionType::Unicode { name: "🔄".to_string() }),
                label: Some("مراقبة السيرفر".to_string()),
                style: btn_style(config.server_edit_monitoring.active),
                url: None,
            }),
        ],
    });

    let r4 = Component::ActionRow(ActionRow {
        components: vec![
            Component::Button(Button {
                custom_id: Some("view_hackMode".to_string()),
                disabled: false,
                emoji: Some(ReactionType::Unicode { name: "💀".to_string() }),
                label: Some("نظام الاختراق".to_string()),
                style: btn_style(config.hack_mode.active),
                url: None,
            }),
        ],
    });

    vec![r1, r2, r3, r4]
}

pub fn create_main_setup_embed(config: &Config) -> twilight_model::channel::message::Embed {
    let builder = EmbedBuilder::new()
        .color(0x0099FF)
        .title("🔒 إعدادات بوت الحماية")
        .description("مرحبًا بك في لوحة تحكم بوت الحماية!\n\n**كيفية الاستخدام:**\n• اضغط على أحد الأزرار أدناه للوصول لإعدادات الحماية المختلفة\n• يمكنك تفعيل/تعطيل أنظمة الحماية بسهولة\n• تظهر الأزرار باللون الأخضر عندما تكون الميزة مفعلة\n• تظهر الأزرار باللون الأحمر عندما تكون الميزة معطلة\n\n**أوامر أخرى مفيدة:**\n• `/setwebhook` - تعيين رابط Webhook لإرسال سجلات الحماية\n• `/whitelist add` - إضافة دور للقائمة البيضاء\n• `/whitelist remove` - إزالة دور من القائمة البيضاء\n• `/whitelist list` - عرض قائمة الأدوار المستثناة")
        .field(
            EmbedFieldBuilder::new(
                "📊 أنظمة الحماية المتوفرة",
                "استخدم الأزرار أدناه للوصول إلى أنظمة الحماية وتخصيص إعداداتها بسهولة",
            )
            .build(),
        );
    apply_embed_footer(builder, &config.embed_footer).build()
}

pub async fn execute(
    state: &AppState,
    interaction: &Interaction,
    _command_data: &CommandData,
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
                content: Some("❌ غير مصرح لك! فقط مالك السيرفر أو الأعضاء في القائمة البيضاء يمكنهم الوصول إلى إعدادات البوت.".to_string()),
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

    let config = state.config.read().clone();
    let main_embed = create_main_setup_embed(&config);
    let components = create_main_setup_components(&config);

    let response = InteractionResponse {
        kind: InteractionResponseType::ChannelMessageWithSource,
        data: Some(twilight_model::http::interaction::InteractionResponseData {
            embeds: Some(vec![main_embed]),
            components: Some(components),
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
