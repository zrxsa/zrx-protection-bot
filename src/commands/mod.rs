pub mod setup;
pub mod setwebhook;
pub mod unhack;
pub mod whitelist;

use twilight_model::application::command::{
    Command, CommandOption, CommandOptionType, CommandType,
};
use twilight_model::application::interaction::application_command::CommandData;
use twilight_model::application::interaction::Interaction;
use twilight_model::id::marker::GuildMarker;
use twilight_model::id::Id;

use crate::state::AppState;

/// Constructs the list of application (slash) commands to register with Discord.
pub fn get_commands() -> Vec<Command> {
    vec![
        // /setwebhook
        Command {
            application_id: None,
            default_member_permissions: None,
            description: "تعيين رابط Webhook لإرسال سجلات وتنبيهات الحماية".to_string(),
            description_localizations: None,
            dm_permission: Some(false),
            guild_id: None,
            id: None,
            kind: CommandType::ChatInput,
            name: "setwebhook".to_string(),
            name_localizations: None,
            nsfw: Some(false),
            options: vec![CommandOption {
                autocomplete: None,
                channel_types: None,
                choices: None,
                description: "رابط Webhook الخاص بالسجلات".to_string(),
                description_localizations: None,
                max_length: None,
                max_value: None,
                min_length: None,
                min_value: None,
                name: "url".to_string(),
                name_localizations: None,
                options: None,
                required: Some(true),
                kind: CommandOptionType::String,
            }],
            version: Id::new(1),
        },
        // /setup
        Command {
            application_id: None,
            default_member_permissions: None,
            description: "عرض إعدادات البوت وتعديلها".to_string(),
            description_localizations: None,
            dm_permission: Some(false),
            guild_id: None,
            id: None,
            kind: CommandType::ChatInput,
            name: "setup".to_string(),
            name_localizations: None,
            nsfw: Some(false),
            options: vec![],
            version: Id::new(1),
        },
        // /unhack
        Command {
            application_id: None,
            default_member_permissions: None,
            description: "إزالة دور المخترق من عضو وإعادة أدواره السابقة".to_string(),
            description_localizations: None,
            dm_permission: Some(false),
            guild_id: None,
            id: None,
            kind: CommandType::ChatInput,
            name: "unhack".to_string(),
            name_localizations: None,
            nsfw: Some(false),
            options: vec![CommandOption {
                autocomplete: None,
                channel_types: None,
                choices: None,
                description: "العضو المراد إزالة دور المخترق منه".to_string(),
                description_localizations: None,
                max_length: None,
                max_value: None,
                min_length: None,
                min_value: None,
                name: "user".to_string(),
                name_localizations: None,
                options: None,
                required: Some(true),
                kind: CommandOptionType::User,
            }],
            version: Id::new(1),
        },
        // /whitelist
        Command {
            application_id: None,
            default_member_permissions: None,
            description: "إدارة الأدوار المستثناة من الحماية".to_string(),
            description_localizations: None,
            dm_permission: Some(false),
            guild_id: None,
            id: None,
            kind: CommandType::ChatInput,
            name: "whitelist".to_string(),
            name_localizations: None,
            nsfw: Some(false),
            options: vec![
                CommandOption {
                    autocomplete: None,
                    channel_types: None,
                    choices: None,
                    description: "إضافة دور للقائمة البيضاء".to_string(),
                    description_localizations: None,
                    max_length: None,
                    max_value: None,
                    min_length: None,
                    min_value: None,
                    name: "add".to_string(),
                    name_localizations: None,
                    options: Some(vec![CommandOption {
                        autocomplete: None,
                        channel_types: None,
                        choices: None,
                        description: "الدور المراد إضافته".to_string(),
                        description_localizations: None,
                        max_length: None,
                        max_value: None,
                        min_length: None,
                        min_value: None,
                        name: "role".to_string(),
                        name_localizations: None,
                        options: None,
                        required: Some(true),
                        kind: CommandOptionType::Role,
                    }]),
                    required: None,
                    kind: CommandOptionType::SubCommand,
                },
                CommandOption {
                    autocomplete: None,
                    channel_types: None,
                    choices: None,
                    description: "إزالة دور من القائمة البيضاء".to_string(),
                    description_localizations: None,
                    max_length: None,
                    max_value: None,
                    min_length: None,
                    min_value: None,
                    name: "remove".to_string(),
                    name_localizations: None,
                    options: Some(vec![CommandOption {
                        autocomplete: None,
                        channel_types: None,
                        choices: None,
                        description: "الدور المراد إزالته".to_string(),
                        description_localizations: None,
                        max_length: None,
                        max_value: None,
                        min_length: None,
                        min_value: None,
                        name: "role".to_string(),
                        name_localizations: None,
                        options: None,
                        required: Some(true),
                        kind: CommandOptionType::Role,
                    }]),
                    required: None,
                    kind: CommandOptionType::SubCommand,
                },
                CommandOption {
                    autocomplete: None,
                    channel_types: None,
                    choices: None,
                    description: "عرض الأدوار المستثناة من الحماية".to_string(),
                    description_localizations: None,
                    max_length: None,
                    max_value: None,
                    min_length: None,
                    min_value: None,
                    name: "list".to_string(),
                    name_localizations: None,
                    options: None,
                    required: None,
                    kind: CommandOptionType::SubCommand,
                },
            ],
            version: Id::new(1),
        },
    ]
}

/// Register slash commands with Discord.
pub async fn register_commands(state: &AppState, application_id: Id<twilight_model::id::marker::ApplicationMarker>) -> anyhow::Result<()> {
    let commands = get_commands();
    let allowed_guild = {
        let cfg = state.config.read();
        cfg.allowed_guild_id.clone()
    };

    if let Ok(gid_raw) = allowed_guild.parse::<u64>() {
        let guild_id = Id::<GuildMarker>::new(gid_raw);
        state
            .http
            .interaction(application_id)
            .set_guild_commands(guild_id, &commands)
            .await?;
        tracing::info!("Registered {} guild commands for guild {}", commands.len(), guild_id);
    } else {
        state
            .http
            .interaction(application_id)
            .set_global_commands(&commands)
            .await?;
        tracing::info!("Registered {} global commands", commands.len());
    }

    Ok(())
}

/// Dispatches application slash command to appropriate handler.
pub async fn handle_command(
    state: &AppState,
    interaction: &Interaction,
    command_data: &CommandData,
) -> anyhow::Result<()> {
    match command_data.name.as_str() {
        "setwebhook" => setwebhook::execute(state, interaction, command_data).await,
        "setup" => setup::execute(state, interaction, command_data).await,
        "unhack" => unhack::execute(state, interaction, command_data).await,
        "whitelist" => whitelist::execute(state, interaction, command_data).await,
        _ => Ok(()),
    }
}
