use twilight_model::application::interaction::application_command::{
    CommandData, CommandDataOption, CommandOptionValue,
};
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::MessageFlags;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

use crate::state::AppState;
use crate::utils::{apply_embed_footer, is_owner_or_whitelisted};

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
                content: Some("❌ غير مصرح لك! فقط مالك السيرفر أو الأعضاء في القائمة البيضاء يمكنهم إدارة الأدوار المستثناة.".to_string()),
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

    if let Some(CommandDataOption {
        name,
        value: CommandOptionValue::SubCommand(options),
    }) = command_data.options.first()
    {
        match name.as_str() {
            "add" => {
                let role_id = options.iter().find_map(|opt| {
                    if opt.name == "role" {
                        if let CommandOptionValue::Role(rid) = opt.value {
                            Some(rid)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });

                if let Some(rid) = role_id {
                    let rid_str = rid.get().to_string();
                    let mut exists = false;
                    {
                        let mut cfg = state.config.write();
                        if cfg.whitelist_roles.contains(&rid_str) {
                            exists = true;
                        } else {
                            cfg.whitelist_roles.push(rid_str.clone());
                        }
                    }

                    if exists {
                        let response = InteractionResponse {
                            kind: InteractionResponseType::ChannelMessageWithSource,
                            data: Some(twilight_model::http::interaction::InteractionResponseData {
                                content: Some(format!("❌ الدور <@&{}> موجود بالفعل في القائمة البيضاء!", rid)),
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

                    let _ = state.save_config().await;
                    let embed_footer = {
                        let cfg = state.config.read();
                        cfg.embed_footer.clone()
                    };

                    let embed = apply_embed_footer(
                        EmbedBuilder::new()
                            .color(0x00FF00)
                            .title("✅ تم إضافة الدور للقائمة البيضاء")
                            .description(format!(
                                "تم إضافة الدور <@&{}> إلى القائمة البيضاء.\nلن يتأثر أعضاء هذا الدور بإجراءات الحماية.",
                                rid
                            )),
                        &embed_footer,
                    )
                    .build();

                    let response = InteractionResponse {
                        kind: InteractionResponseType::ChannelMessageWithSource,
                        data: Some(twilight_model::http::interaction::InteractionResponseData {
                            embeds: Some(vec![embed]),
                            flags: Some(MessageFlags::EPHEMERAL),
                            ..Default::default()
                        }),
                    };
                    state
                        .http
                        .interaction(interaction.application_id)
                        .create_response(interaction.id, &interaction.token, &response)
                        .await?;
                }
            }
            "remove" => {
                let role_id = options.iter().find_map(|opt| {
                    if opt.name == "role" {
                        if let CommandOptionValue::Role(rid) = opt.value {
                            Some(rid)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });

                if let Some(rid) = role_id {
                    let rid_str = rid.get().to_string();
                    let mut removed = false;
                    {
                        let mut cfg = state.config.write();
                        if let Some(pos) = cfg.whitelist_roles.iter().position(|x| x == &rid_str) {
                            cfg.whitelist_roles.remove(pos);
                            removed = true;
                        }
                    }

                    if !removed {
                        let response = InteractionResponse {
                            kind: InteractionResponseType::ChannelMessageWithSource,
                            data: Some(twilight_model::http::interaction::InteractionResponseData {
                                content: Some(format!("❌ الدور <@&{}> غير موجود في القائمة البيضاء!", rid)),
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

                    let _ = state.save_config().await;
                    let embed_footer = {
                        let cfg = state.config.read();
                        cfg.embed_footer.clone()
                    };

                    let embed = apply_embed_footer(
                        EmbedBuilder::new()
                            .color(0xFF0000)
                            .title("✅ تم إزالة الدور من القائمة البيضاء")
                            .description(format!(
                                "تم إزالة الدور <@&{}> من القائمة البيضاء.\nسيتأثر أعضاء هذا الدور بإجراءات الحماية.",
                                rid
                            )),
                        &embed_footer,
                    )
                    .build();

                    let response = InteractionResponse {
                        kind: InteractionResponseType::ChannelMessageWithSource,
                        data: Some(twilight_model::http::interaction::InteractionResponseData {
                            embeds: Some(vec![embed]),
                            flags: Some(MessageFlags::EPHEMERAL),
                            ..Default::default()
                        }),
                    };
                    state
                        .http
                        .interaction(interaction.application_id)
                        .create_response(interaction.id, &interaction.token, &response)
                        .await?;
                }
            }
            "list" => {
                let (whitelist, embed_footer) = {
                    let cfg = state.config.read();
                    (cfg.whitelist_roles.clone(), cfg.embed_footer.clone())
                };

                let mut roles_list = String::new();
                let mut count = 0;

                for role_id_str in &whitelist {
                    if let Ok(rid) = role_id_str.parse::<u64>() {
                        count += 1;
                        roles_list.push_str(&format!("{}. <@&{}>\n", count, rid));
                    }
                }

                if count == 0 {
                    roles_list = "لا توجد أدوار مضافة للقائمة البيضاء.".to_string();
                }

                let embed = apply_embed_footer(
                    EmbedBuilder::new()
                        .color(0x0099FF)
                        .title("📋 قائمة الأدوار المستثناة من الحماية")
                        .description(roles_list)
                        .field(
                            EmbedFieldBuilder::new(
                                "ℹ️ معلومات",
                                "أعضاء هذه الأدوار لن يتأثروا بإجراءات الحماية التلقائية.",
                            )
                            .build(),
                        ),
                    &embed_footer,
                )
                .build();

                let response = InteractionResponse {
                    kind: InteractionResponseType::ChannelMessageWithSource,
                    data: Some(twilight_model::http::interaction::InteractionResponseData {
                        embeds: Some(vec![embed]),
                        flags: Some(MessageFlags::EPHEMERAL),
                        ..Default::default()
                    }),
                };
                state
                    .http
                    .interaction(interaction.application_id)
                    .create_response(interaction.id, &interaction.token, &response)
                    .await?;
            }
            _ => {}
        }
    }

    Ok(())
}
