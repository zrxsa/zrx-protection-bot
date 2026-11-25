use std::sync::Arc;
use twilight_model::application::interaction::message_component::MessageComponentInteractionData;
use twilight_model::application::interaction::{Interaction, InteractionData, InteractionType};
use twilight_model::channel::message::component::{
    ActionRow, Button, ButtonStyle, Component, SelectMenu, SelectMenuOption,
};
use twilight_model::channel::message::MessageFlags;
use twilight_model::channel::message::ReactionType;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

use crate::commands::setup::{create_main_setup_components, create_main_setup_embed};
use crate::config::Config;
use crate::state::AppState;
use crate::utils::{apply_embed_footer, is_owner_or_whitelisted};

pub async fn handle_interaction(state: Arc<AppState>, interaction: Interaction) {
    let guild_id = match interaction.guild_id {
        Some(gid) => gid,
        None => return,
    };

    let user_id = match interaction.author_id() {
        Some(uid) => uid,
        None => return,
    };

    let member_roles = interaction
        .member
        .as_ref()
        .map(|m| m.roles.as_slice());

    match interaction.kind {
        InteractionType::ApplicationCommand => {
            if let Some(InteractionData::ApplicationCommand(ref command_data)) = interaction.data {
                if let Err(e) = crate::commands::handle_command(&state, &interaction, command_data).await {
                    tracing::error!("Error executing command {}: {:?}", command_data.name, e);
                }
            }
        }
        InteractionType::MessageComponent => {
            if !is_owner_or_whitelisted(&state, guild_id, user_id, member_roles).await {
                let response = InteractionResponse {
                    kind: InteractionResponseType::ChannelMessageWithSource,
                    data: Some(twilight_model::http::interaction::InteractionResponseData {
                        content: Some("❌ غير مصرح لك! فقط مالك السيرفر أو الأعضاء في القائمة البيضاء يمكنهم الوصول إلى هذه الإعدادات.".to_string()),
                        flags: Some(MessageFlags::EPHEMERAL),
                        ..Default::default()
                    }),
                };
                let _ = state
                    .http
                    .interaction(interaction.application_id)
                    .create_response(interaction.id, &interaction.token, &response)
                    .await;
                return;
            }

            if let Some(InteractionData::MessageComponent(ref comp_data)) = interaction.data {
                handle_component_interaction(&state, &interaction, comp_data).await;
            }
        }
        _ => {}
    }
}

async fn handle_component_interaction(
    state: &AppState,
    interaction: &Interaction,
    comp_data: &MessageComponentInteractionData,
) {
    let custom_id = comp_data.custom_id.as_str();

    // 1. Back to main
    if custom_id == "back_to_main" {
        let config = state.config.read().clone();
        let main_embed = create_main_setup_embed(&config);
        let components = create_main_setup_components(&config);

        let response = InteractionResponse {
            kind: InteractionResponseType::UpdateMessage,
            data: Some(twilight_model::http::interaction::InteractionResponseData {
                embeds: Some(vec![main_embed]),
                components: Some(components),
                ..Default::default()
            }),
        };
        let _ = state
            .http
            .interaction(interaction.application_id)
            .create_response(interaction.id, &interaction.token, &response)
            .await;
        return;
    }

    // 2. View category settings
    if let Some(category) = custom_id.strip_prefix("view_") {
        let config = state.config.read().clone();
        let embed = create_category_settings_embed(category, &config);
        let components = create_category_settings_components(category, &config);

        let response = InteractionResponse {
            kind: InteractionResponseType::UpdateMessage,
            data: Some(twilight_model::http::interaction::InteractionResponseData {
                embeds: Some(vec![embed]),
                components: Some(components),
                ..Default::default()
            }),
        };
        let _ = state
            .http
            .interaction(interaction.application_id)
            .create_response(interaction.id, &interaction.token, &response)
            .await;
        return;
    }

    // 3. Category toggles
    let is_category_toggle = matches!(
        custom_id,
        "antispam"
            | "antilinks"
            | "channelMonitoring"
            | "roleMonitoring"
            | "kickMonitoring"
            | "banMonitoring"
            | "botMonitoring"
            | "roleEditMonitoring"
            | "serverEditMonitoring"
            | "hackMode"
    );

    if is_category_toggle {
        {
            let mut cfg = state.config.write();
            match custom_id {
                "antispam" => cfg.antispam.active = !cfg.antispam.active,
                "antilinks" => cfg.antilinks.active = !cfg.antilinks.active,
                "channelMonitoring" => cfg.channel_monitoring.active = !cfg.channel_monitoring.active,
                "roleMonitoring" => cfg.role_monitoring.active = !cfg.role_monitoring.active,
                "kickMonitoring" => cfg.kick_monitoring.active = !cfg.kick_monitoring.active,
                "banMonitoring" => cfg.ban_monitoring.active = !cfg.ban_monitoring.active,
                "botMonitoring" => cfg.bot_monitoring.active = !cfg.bot_monitoring.active,
                "roleEditMonitoring" => cfg.role_edit_monitoring.active = !cfg.role_edit_monitoring.active,
                "serverEditMonitoring" => cfg.server_edit_monitoring.active = !cfg.server_edit_monitoring.active,
                "hackMode" => cfg.hack_mode.active = !cfg.hack_mode.active,
                _ => {}
            }
        }
        let _ = state.save_config().await;

        let config = state.config.read().clone();
        let embed = create_category_settings_embed(custom_id, &config);
        let components = create_category_settings_components(custom_id, &config);

        let response = InteractionResponse {
            kind: InteractionResponseType::UpdateMessage,
            data: Some(twilight_model::http::interaction::InteractionResponseData {
                embeds: Some(vec![embed]),
                components: Some(components),
                ..Default::default()
            }),
        };
        if let Err(e) = state
            .http
            .interaction(interaction.application_id)
            .create_response(interaction.id, &interaction.token, &response)
            .await
        {
            tracing::warn!("Failed to send category toggle response: {:?}", e);
        }
        return;
    }

    // 4. Sub-toggles for antispam / antilinks
    let mut updated_category = None;
    {
        let mut cfg = state.config.write();
        match custom_id {
            "antispam_toggle_delete" => {
                cfg.antispam.delete_messages = !cfg.antispam.delete_messages;
                updated_category = Some("antispam");
            }
            "antilinks_toggle_delete" => {
                cfg.antilinks.delete_messages = !cfg.antilinks.delete_messages;
                updated_category = Some("antilinks");
            }
            "antilinks_toggle_timeout" => {
                cfg.antilinks.actions.timeout = !cfg.antilinks.actions.timeout;
                updated_category = Some("antilinks");
            }
            "antilinks_toggle_kick" => {
                cfg.antilinks.actions.kick = !cfg.antilinks.actions.kick;
                updated_category = Some("antilinks");
            }
            "antilinks_toggle_ban" => {
                cfg.antilinks.actions.ban = !cfg.antilinks.actions.ban;
                updated_category = Some("antilinks");
            }
            _ => {}
        }
    }

    if let Some(cat) = updated_category {
        let _ = state.save_config().await;
        let config = state.config.read().clone();
        let embed = create_category_settings_embed(cat, &config);
        let components = create_category_settings_components(cat, &config);

        let response = InteractionResponse {
            kind: InteractionResponseType::UpdateMessage,
            data: Some(twilight_model::http::interaction::InteractionResponseData {
                embeds: Some(vec![embed]),
                components: Some(components),
                ..Default::default()
            }),
        };
        if let Err(e) = state
            .http
            .interaction(interaction.application_id)
            .create_response(interaction.id, &interaction.token, &response)
            .await
        {
            tracing::warn!("Failed to send sub-toggle response: {:?}", e);
        }
        return;
    }

    // 5. String Select Menus (`setting_<category>.<field>`)
    if let Some(setting_key) = custom_id.strip_prefix("setting_") {
        if let Some(selected_val) = comp_data.values.first() {
            let cat = apply_select_setting(state, setting_key, selected_val);
            let _ = state.save_config().await;

            let config = state.config.read().clone();
            let embed = create_category_settings_embed(&cat, &config);
            let components = create_category_settings_components(&cat, &config);

            let response = InteractionResponse {
                kind: InteractionResponseType::UpdateMessage,
                data: Some(twilight_model::http::interaction::InteractionResponseData {
                    embeds: Some(vec![embed]),
                    components: Some(components),
                    ..Default::default()
                }),
            };
            if let Err(e) = state
                .http
                .interaction(interaction.application_id)
                .create_response(interaction.id, &interaction.token, &response)
                .await
            {
                tracing::warn!("Failed to send select menu update response: {:?}", e);
            }
        }
    }
}

fn apply_select_setting(state: &AppState, key: &str, value: &str) -> String {
    let mut cfg = state.config.write();
    let parts: Vec<&str> = key.split('.').collect();
    let category = parts[0];

    match key {
        "antispam.messageCount" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.antispam.message_count = v;
            }
        }
        "antispam.time" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.antispam.time = v;
            }
        }
        "antispam.action" => {
            cfg.antispam.action = value.to_string();
        }
        "antilinks.actions.timeoutDuration" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.antilinks.actions.timeout_duration = v;
            }
        }
        "channelMonitoring.action" => cfg.channel_monitoring.action = value.to_string(),
        "channelMonitoring.maxChannelChanges" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.channel_monitoring.max_channel_changes = v;
            }
        }
        "channelMonitoring.timeFrame" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.channel_monitoring.time_frame = v;
            }
        }
        "roleMonitoring.action" => cfg.role_monitoring.action = value.to_string(),
        "roleMonitoring.maxRoleChanges" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.role_monitoring.max_role_changes = v;
            }
        }
        "roleMonitoring.timeFrame" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.role_monitoring.time_frame = v;
            }
        }
        "kickMonitoring.action" => cfg.kick_monitoring.action = value.to_string(),
        "kickMonitoring.maxKicks" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.kick_monitoring.max_kicks = v;
            }
        }
        "kickMonitoring.timeFrame" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.kick_monitoring.time_frame = v;
            }
        }
        "banMonitoring.action" => cfg.ban_monitoring.action = value.to_string(),
        "banMonitoring.maxBans" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.ban_monitoring.max_bans = v;
            }
        }
        "banMonitoring.timeFrame" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.ban_monitoring.time_frame = v;
            }
        }
        "botMonitoring.action" => cfg.bot_monitoring.action = value.to_string(),
        "botMonitoring.botAction" => cfg.bot_monitoring.bot_action = value.to_string(),
        "roleEditMonitoring.action" => cfg.role_edit_monitoring.action = value.to_string(),
        "roleEditMonitoring.maxEdits" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.role_edit_monitoring.max_edits = v;
            }
        }
        "roleEditMonitoring.timeFrame" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.role_edit_monitoring.time_frame = v;
            }
        }
        "serverEditMonitoring.action" => cfg.server_edit_monitoring.action = value.to_string(),
        "serverEditMonitoring.maxEdits" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.server_edit_monitoring.max_edits = v;
            }
        }
        "serverEditMonitoring.timeFrame" => {
            if let Ok(v) = value.parse::<u64>() {
                cfg.server_edit_monitoring.time_frame = v;
            }
        }
        _ => {}
    }

    category.to_string()
}

fn create_category_settings_embed(category: &str, config: &Config) -> twilight_model::channel::message::Embed {
    let mut builder = apply_embed_footer(
        EmbedBuilder::new()
            .color(0x0099FF)
            .title(format!("⚙️ إعدادات {}", get_arabic_name(category))),
        &config.embed_footer,
    );

    match category {
        "antispam" => {
            builder = builder
                .description("**🛡️ نظام مكافحة الرسائل المزعجة**\n\nيقوم بمراقبة سرعة إرسال الرسائل من المستخدمين. إذا تجاوز عدد الرسائل الحد المسموح في الوقت المحدد، سيتم اتخاذ الإجراء المناسب لمنع الإغراق.")
                .field(EmbedFieldBuilder::new("🔄 الحالة", if config.antispam.active { "✅ مفعل" } else { "❌ معطل" }).inline().build())
                .field(EmbedFieldBuilder::new("📊 عدد الرسائل المسموح", format!("{}", config.antispam.message_count)).inline().build())
                .field(EmbedFieldBuilder::new("⏱️ الفترة الزمنية", format!("{}ms", config.antispam.time)).inline().build())
                .field(EmbedFieldBuilder::new("🗑️ حذف الرسائل", if config.antispam.delete_messages { "✅ نعم" } else { "❌ لا" }).inline().build())
                .field(EmbedFieldBuilder::new("⚠️ نوع الإجراء", get_action_arabic_name(&config.antispam.action)).inline().build())
                .field(EmbedFieldBuilder::new("⌛ مدة المهلة", format!("{}ms", config.antispam.timeout_duration)).inline().build());
        }
        "antilinks" => {
            builder = builder
                .description("**🔗 نظام مكافحة الروابط**\n\nيقوم بمنع نشر الروابط في السيرفر للحماية من الإعلانات والمحتوى غير المرغوب فيه.")
                .field(EmbedFieldBuilder::new("🔄 الحالة", if config.antilinks.active { "✅ مفعل" } else { "❌ معطل" }).inline().build())
                .field(EmbedFieldBuilder::new("🗑️ حذف الرسائل", if config.antilinks.delete_messages { "✅ نعم" } else { "❌ لا" }).inline().build())
                .field(EmbedFieldBuilder::new("⏳ المهلة المؤقتة", if config.antilinks.actions.timeout { "✅ مفعلة" } else { "❌ معطلة" }).inline().build())
                .field(EmbedFieldBuilder::new("⌛ مدة المهلة", format!("{}ms", config.antilinks.actions.timeout_duration)).inline().build())
                .field(EmbedFieldBuilder::new("👢 الطرد", if config.antilinks.actions.kick { "✅ مفعل" } else { "❌ معطل" }).inline().build())
                .field(EmbedFieldBuilder::new("🚫 الحظر", if config.antilinks.actions.ban { "✅ مفعل" } else { "❌ معطل" }).inline().build());
        }
        "channelMonitoring" => {
            builder = builder
                .description("**📝 نظام حماية القنوات**\n\nيراقب التغييرات على قنوات السيرفر مثل الإنشاء والحذف.")
                .field(EmbedFieldBuilder::new("🔄 الحالة", if config.channel_monitoring.active { "✅ مفعل" } else { "❌ معطل" }).inline().build())
                .field(EmbedFieldBuilder::new("📊 الحد الأقصى للتغييرات", format!("{}", config.channel_monitoring.max_channel_changes)).inline().build())
                .field(EmbedFieldBuilder::new("⏱️ الإطار الزمني", format!("{}ms", config.channel_monitoring.time_frame)).inline().build())
                .field(EmbedFieldBuilder::new("⚠️ نوع الإجراء", get_action_arabic_name(&config.channel_monitoring.action)).inline().build())
                .field(EmbedFieldBuilder::new("⌛ مدة المهلة", format!("{}ms", config.channel_monitoring.timeout_duration)).inline().build());
        }
        "roleMonitoring" => {
            builder = builder
                .description("**👑 نظام حماية الأدوار**\n\nيراقب إنشاء وحذف أدوار السيرفر.")
                .field(EmbedFieldBuilder::new("🔄 الحالة", if config.role_monitoring.active { "✅ مفعل" } else { "❌ معطل" }).inline().build())
                .field(EmbedFieldBuilder::new("📊 الحد الأقصى للتغييرات", format!("{}", config.role_monitoring.max_role_changes)).inline().build())
                .field(EmbedFieldBuilder::new("⏱️ الإطار الزمني", format!("{}ms", config.role_monitoring.time_frame)).inline().build())
                .field(EmbedFieldBuilder::new("⚠️ نوع الإجراء", get_action_arabic_name(&config.role_monitoring.action)).inline().build())
                .field(EmbedFieldBuilder::new("⌛ مدة المهلة", format!("{}ms", config.role_monitoring.timeout_duration)).inline().build());
        }
        "kickMonitoring" => {
            builder = builder
                .description("**🚫 نظام مراقبة عمليات الطرد**\n\nيراقب عمليات طرد الأعضاء ويمنع الطرد الجماعي.")
                .field(EmbedFieldBuilder::new("🔄 الحالة", if config.kick_monitoring.active { "✅ مفعل" } else { "❌ معطل" }).inline().build())
                .field(EmbedFieldBuilder::new("📊 الحد الأقصى للطرد", format!("{}", config.kick_monitoring.max_kicks)).inline().build())
                .field(EmbedFieldBuilder::new("⏱️ الإطار الزمني", format!("{}ms", config.kick_monitoring.time_frame)).inline().build())
                .field(EmbedFieldBuilder::new("⚠️ نوع الإجراء", get_action_arabic_name(&config.kick_monitoring.action)).inline().build())
                .field(EmbedFieldBuilder::new("⌛ مدة المهلة", format!("{}ms", config.kick_monitoring.timeout_duration)).inline().build());
        }
        "banMonitoring" => {
            builder = builder
                .description("**⛔ نظام مراقبة عمليات الحظر**\n\nيراقب عمليات حظر الأعضاء ويمنع الحظر الجماعي.")
                .field(EmbedFieldBuilder::new("🔄 الحالة", if config.ban_monitoring.active { "✅ مفعل" } else { "❌ معطل" }).inline().build())
                .field(EmbedFieldBuilder::new("📊 الحد الأقصى للحظر", format!("{}", config.ban_monitoring.max_bans)).inline().build())
                .field(EmbedFieldBuilder::new("⏱️ الإطار الزمني", format!("{}ms", config.ban_monitoring.time_frame)).inline().build())
                .field(EmbedFieldBuilder::new("⚠️ نوع الإجراء", get_action_arabic_name(&config.ban_monitoring.action)).inline().build())
                .field(EmbedFieldBuilder::new("⌛ مدة المهلة", format!("{}ms", config.ban_monitoring.timeout_duration)).inline().build());
        }
        "botMonitoring" => {
            builder = builder
                .description("**🤖 نظام حماية ضد البوتات**\n\nيراقب إضافة البوتات غير المصرح بها.")
                .field(EmbedFieldBuilder::new("🔄 الحالة", if config.bot_monitoring.active { "✅ مفعل" } else { "❌ معطل" }).inline().build())
                .field(EmbedFieldBuilder::new("⚠️ إجراء المستخدم", get_action_arabic_name(&config.bot_monitoring.action)).inline().build())
                .field(EmbedFieldBuilder::new("🤖 إجراء البوت", get_action_arabic_name(&config.bot_monitoring.bot_action)).inline().build())
                .field(EmbedFieldBuilder::new("⌛ مدة المهلة", format!("{}ms", config.bot_monitoring.timeout_duration)).inline().build());
        }
        "roleEditMonitoring" => {
            builder = builder
                .description("**✏️ نظام مراقبة تعديل الأدوار**\n\nيراقب تعديل الصلاحيات وتعيين الأدوار.")
                .field(EmbedFieldBuilder::new("🔄 الحالة", if config.role_edit_monitoring.active { "✅ مفعل" } else { "❌ معطل" }).inline().build())
                .field(EmbedFieldBuilder::new("📊 الحد الأقصى للتعديلات", format!("{}", config.role_edit_monitoring.max_edits)).inline().build())
                .field(EmbedFieldBuilder::new("⏱️ الإطار الزمني", format!("{}ms", config.role_edit_monitoring.time_frame)).inline().build())
                .field(EmbedFieldBuilder::new("⚠️ نوع الإجراء", get_action_arabic_name(&config.role_edit_monitoring.action)).inline().build())
                .field(EmbedFieldBuilder::new("⌛ مدة المهلة", format!("{}ms", config.role_edit_monitoring.timeout_duration)).inline().build());
        }
        "serverEditMonitoring" => {
            builder = builder
                .description("**🔄 نظام مراقبة تعديل السيرفر**\n\nيراقب التغييرات على إعدادات السيرفر الأساسية.")
                .field(EmbedFieldBuilder::new("🔄 الحالة", if config.server_edit_monitoring.active { "✅ مفعل" } else { "❌ معطل" }).inline().build())
                .field(EmbedFieldBuilder::new("📊 الحد الأقصى للتعديلات", format!("{}", config.server_edit_monitoring.max_edits)).inline().build())
                .field(EmbedFieldBuilder::new("⏱️ الإطار الزمني", format!("{}ms", config.server_edit_monitoring.time_frame)).inline().build())
                .field(EmbedFieldBuilder::new("⚠️ نوع الإجراء", get_action_arabic_name(&config.server_edit_monitoring.action)).inline().build())
                .field(EmbedFieldBuilder::new("⌛ مدة المهلة", format!("{}ms", config.server_edit_monitoring.timeout_duration)).inline().build());
        }
        "hackMode" => {
            let format_channel = |id: &str| {
                let trimmed = id.trim();
                if trimmed.is_empty() {
                    "غير محدد".to_string()
                } else {
                    format!("<#{}>", trimmed)
                }
            };
            let format_role = |id: &str| {
                let trimmed = id.trim();
                if trimmed.is_empty() {
                    "غير محدد".to_string()
                } else {
                    format!("<@&{}>", trimmed)
                }
            };
            builder = builder
                .description("**💀 نظام الاختراق**\n\nسحب رتب الأعضاء الذين يرسلون رسائل في قناة الاختراق وعزلهم.")
                .field(EmbedFieldBuilder::new("🔄 الحالة", if config.hack_mode.active { "✅ مفعل" } else { "❌ معطل" }).inline().build())
                .field(EmbedFieldBuilder::new("💬 قناة الاختراق", format_channel(&config.hack_mode.hack_channel_id)).inline().build())
                .field(EmbedFieldBuilder::new("👤 دور المخترق", format_role(&config.hack_mode.hacked_role_id)).inline().build())
                .field(EmbedFieldBuilder::new("👁️ القناة المسموح بها", format_channel(&config.hack_mode.hacked_view_channel_id)).inline().build());
        }
        _ => {}
    }

    builder.build()
}

fn create_category_settings_components(category: &str, config: &Config) -> Vec<Component> {
    let is_active = match category {
        "antispam" => config.antispam.active,
        "antilinks" => config.antilinks.active,
        "channelMonitoring" => config.channel_monitoring.active,
        "roleMonitoring" => config.role_monitoring.active,
        "kickMonitoring" => config.kick_monitoring.active,
        "banMonitoring" => config.ban_monitoring.active,
        "botMonitoring" => config.bot_monitoring.active,
        "roleEditMonitoring" => config.role_edit_monitoring.active,
        "serverEditMonitoring" => config.server_edit_monitoring.active,
        "hackMode" => config.hack_mode.active,
        _ => false,
    };

    let toggle_row = Component::ActionRow(ActionRow {
        components: vec![
            Component::Button(Button {
                custom_id: Some(category.to_string()),
                disabled: false,
                emoji: Some(ReactionType::Unicode {
                    name: if is_active { "🔴".to_string() } else { "🟢".to_string() },
                }),
                label: Some(if is_active { "تعطيل".to_string() } else { "تفعيل".to_string() }),
                style: if is_active { ButtonStyle::Danger } else { ButtonStyle::Success },
                url: None,
            }),
            Component::Button(Button {
                custom_id: Some("back_to_main".to_string()),
                disabled: false,
                emoji: Some(ReactionType::Unicode { name: "🔙".to_string() }),
                label: Some("العودة للقائمة الرئيسية".to_string()),
                style: ButtonStyle::Primary,
                url: None,
            }),
        ],
    });

    let mut rows = vec![toggle_row];

    match category {
        "antispam" => {
            let delete_btn = Component::ActionRow(ActionRow {
                components: vec![Component::Button(Button {
                    custom_id: Some("antispam_toggle_delete".to_string()),
                    disabled: false,
                    emoji: Some(ReactionType::Unicode { name: "🗑️".to_string() }),
                    label: Some(if config.antispam.delete_messages {
                        "إيقاف حذف الرسائل".to_string()
                    } else {
                        "تفعيل حذف الرسائل".to_string()
                    }),
                    style: ButtonStyle::Secondary,
                    url: None,
                })],
            });
            rows.push(delete_btn);

            let count_menu = Component::ActionRow(ActionRow {
                components: vec![Component::SelectMenu(SelectMenu {
                    custom_id: "setting_antispam.messageCount".to_string(),
                    disabled: false,
                    max_values: Some(1),
                    min_values: Some(1),
                    options: vec![
                        SelectMenuOption { default: false, description: Some("حماية قوية جداً".to_string()), emoji: None, label: "3 رسائل".to_string(), value: "3".to_string() },
                        SelectMenuOption { default: false, description: Some("حماية قوية".to_string()), emoji: None, label: "5 رسائل".to_string(), value: "5".to_string() },
                        SelectMenuOption { default: false, description: Some("حماية متوسطة".to_string()), emoji: None, label: "7 رسائل".to_string(), value: "7".to_string() },
                        SelectMenuOption { default: false, description: Some("حماية منخفضة".to_string()), emoji: None, label: "10 رسائل".to_string(), value: "10".to_string() },
                    ],
                    placeholder: Some("تعيين عدد الرسائل المسموح".to_string()),
                })],
            });
            rows.push(count_menu);

            let action_menu = Component::ActionRow(ActionRow {
                components: vec![Component::SelectMenu(SelectMenu {
                    custom_id: "setting_antispam.action".to_string(),
                    disabled: false,
                    max_values: Some(1),
                    min_values: Some(1),
                    options: vec![
                        SelectMenuOption { default: false, description: Some("وضع المستخدم في مهلة مؤقتة".to_string()), emoji: None, label: "مهلة مؤقتة".to_string(), value: "timeout".to_string() },
                        SelectMenuOption { default: false, description: Some("طرد المستخدم من السيرفر".to_string()), emoji: None, label: "طرد".to_string(), value: "kick".to_string() },
                        SelectMenuOption { default: false, description: Some("حظر المستخدم من السيرفر".to_string()), emoji: None, label: "حظر".to_string(), value: "ban".to_string() },
                        SelectMenuOption { default: false, description: Some("حذف الرسائل فقط".to_string()), emoji: None, label: "بدون إجراء".to_string(), value: "none".to_string() },
                    ],
                    placeholder: Some("تحديد نوع الإجراء".to_string()),
                })],
            });
            rows.push(action_menu);
        }
        "antilinks" => {
            let row1 = Component::ActionRow(ActionRow {
                components: vec![
                    Component::Button(Button {
                        custom_id: Some("antilinks_toggle_delete".to_string()),
                        disabled: false,
                        emoji: Some(ReactionType::Unicode { name: "🗑️".to_string() }),
                        label: Some(if config.antilinks.delete_messages { "إيقاف حذف الرسائل".to_string() } else { "تفعيل حذف الرسائل".to_string() }),
                        style: ButtonStyle::Secondary,
                        url: None,
                    }),
                    Component::Button(Button {
                        custom_id: Some("antilinks_toggle_timeout".to_string()),
                        disabled: false,
                        emoji: Some(ReactionType::Unicode { name: "⏳".to_string() }),
                        label: Some(if config.antilinks.actions.timeout { "إيقاف المهلة المؤقتة".to_string() } else { "تفعيل المهلة المؤقتة".to_string() }),
                        style: ButtonStyle::Secondary,
                        url: None,
                    }),
                ],
            });
            rows.push(row1);

            let row2 = Component::ActionRow(ActionRow {
                components: vec![
                    Component::Button(Button {
                        custom_id: Some("antilinks_toggle_kick".to_string()),
                        disabled: false,
                        emoji: Some(ReactionType::Unicode { name: "👢".to_string() }),
                        label: Some(if config.antilinks.actions.kick { "إيقاف الطرد".to_string() } else { "تفعيل الطرد".to_string() }),
                        style: ButtonStyle::Secondary,
                        url: None,
                    }),
                    Component::Button(Button {
                        custom_id: Some("antilinks_toggle_ban".to_string()),
                        disabled: false,
                        emoji: Some(ReactionType::Unicode { name: "🚫".to_string() }),
                        label: Some(if config.antilinks.actions.ban { "إيقاف الحظر".to_string() } else { "تفعيل الحظر".to_string() }),
                        style: ButtonStyle::Secondary,
                        url: None,
                    }),
                ],
            });
            rows.push(row2);
        }
        "channelMonitoring"
        | "roleMonitoring"
        | "kickMonitoring"
        | "banMonitoring"
        | "roleEditMonitoring"
        | "serverEditMonitoring" => {
            let max_field = match category {
                "kickMonitoring" => "maxKicks",
                "banMonitoring" => "maxBans",
                "roleEditMonitoring" | "serverEditMonitoring" => "maxEdits",
                "channelMonitoring" => "maxChannelChanges",
                _ => "maxRoleChanges",
            };

            let action_menu = Component::ActionRow(ActionRow {
                components: vec![Component::SelectMenu(SelectMenu {
                    custom_id: format!("setting_{}.action", category),
                    disabled: false,
                    max_values: Some(1),
                    min_values: Some(1),
                    options: vec![
                        SelectMenuOption { default: false, description: Some("وضع المستخدم في مهلة مؤقتة".to_string()), emoji: None, label: "مهلة مؤقتة".to_string(), value: "timeout".to_string() },
                        SelectMenuOption { default: false, description: Some("طرد المستخدم من السيرفر".to_string()), emoji: None, label: "طرد".to_string(), value: "kick".to_string() },
                        SelectMenuOption { default: false, description: Some("حظر المستخدم من السيرفر".to_string()), emoji: None, label: "حظر".to_string(), value: "ban".to_string() },
                    ],
                    placeholder: Some("تحديد نوع الإجراء".to_string()),
                })],
            });
            rows.push(action_menu);

            let max_menu = Component::ActionRow(ActionRow {
                components: vec![Component::SelectMenu(SelectMenu {
                    custom_id: format!("setting_{}.{}", category, max_field),
                    disabled: false,
                    max_values: Some(1),
                    min_values: Some(1),
                    options: vec![
                        SelectMenuOption { default: false, description: Some("حماية قوية جداً".to_string()), emoji: None, label: "1".to_string(), value: "1".to_string() },
                        SelectMenuOption { default: false, description: Some("حماية قوية".to_string()), emoji: None, label: "2".to_string(), value: "2".to_string() },
                        SelectMenuOption { default: false, description: Some("حماية متوسطة".to_string()), emoji: None, label: "3".to_string(), value: "3".to_string() },
                        SelectMenuOption { default: false, description: Some("حماية منخفضة".to_string()), emoji: None, label: "5".to_string(), value: "5".to_string() },
                    ],
                    placeholder: Some("تحديد الحد الأقصى".to_string()),
                })],
            });
            rows.push(max_menu);
        }
        "botMonitoring" => {
            let user_action_menu = Component::ActionRow(ActionRow {
                components: vec![Component::SelectMenu(SelectMenu {
                    custom_id: "setting_botMonitoring.action".to_string(),
                    disabled: false,
                    max_values: Some(1),
                    min_values: Some(1),
                    options: vec![
                        SelectMenuOption { default: false, description: Some("وضع المستخدم في مهلة مؤقتة".to_string()), emoji: None, label: "مهلة مؤقتة".to_string(), value: "timeout".to_string() },
                        SelectMenuOption { default: false, description: Some("طرد المستخدم من السيرفر".to_string()), emoji: None, label: "طرد".to_string(), value: "kick".to_string() },
                        SelectMenuOption { default: false, description: Some("حظر المستخدم من السيرفر".to_string()), emoji: None, label: "حظر".to_string(), value: "ban".to_string() },
                    ],
                    placeholder: Some("تحديد إجراء المستخدم".to_string()),
                })],
            });
            rows.push(user_action_menu);

            let bot_action_menu = Component::ActionRow(ActionRow {
                components: vec![Component::SelectMenu(SelectMenu {
                    custom_id: "setting_botMonitoring.botAction".to_string(),
                    disabled: false,
                    max_values: Some(1),
                    min_values: Some(1),
                    options: vec![
                        SelectMenuOption { default: false, description: Some("عدم اتخاذ إجراء ضد البوت".to_string()), emoji: None, label: "لا شيء".to_string(), value: "none".to_string() },
                        SelectMenuOption { default: false, description: Some("طرد البوت من السيرفر".to_string()), emoji: None, label: "طرد".to_string(), value: "kick".to_string() },
                    ],
                    placeholder: Some("تحديد إجراء البوت".to_string()),
                })],
            });
            rows.push(bot_action_menu);
        }
        _ => {}
    }

    rows
}

fn get_arabic_name(category: &str) -> &'static str {
    match category {
        "antispam" => "مكافحة السبام",
        "antilinks" => "مكافحة الروابط",
        "channelMonitoring" => "حماية القنوات",
        "roleMonitoring" => "حماية الأدوار",
        "kickMonitoring" => "مراقبة الطرد",
        "banMonitoring" => "مراقبة الحظر",
        "botMonitoring" => "حماية البوتات",
        "roleEditMonitoring" => "مراقبة تعديل الأدوار",
        "serverEditMonitoring" => "مراقبة السيرفر",
        "hackMode" => "نظام الاختراق",
        _ => "الحماية",
    }
}

fn get_action_arabic_name(action: &str) -> &'static str {
    match action {
        "timeout" => "مهلة مؤقتة",
        "kick" => "طرد",
        "ban" => "حظر",
        "none" => "لا شيء",
        _ => "لا شيء",
    }
}
