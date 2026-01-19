use std::time::Duration;

use poise::CreateReply;
use serenity::all::ComponentInteractionCollector;
use serenity::all::ComponentInteractionDataKind;
use serenity::all::CreateActionRow;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateSelectMenu;
use serenity::all::CreateSelectMenuKind;
use serenity::all::CreateSelectMenuOption;
use serenity::all::CreateTextDisplay;
use serenity::all::MessageFlags;
use serenity::all::UserId;

use crate::bot::cog::Context;
use crate::bot::cog::Error;
use crate::bot::error::BotError;
use crate::database::model::ServerSettings;

pub struct VoiceCog;

impl VoiceCog {
    #[poise::command(
        slash_command,
        guild_only,
        subcommands("Self::settings", "Self::leaderboard", "Self::history")
    )]
    pub async fn vc(_ctx: Context<'_>) -> Result<(), Error> {
        Ok(())
    }

    #[poise::command(
        slash_command,
        guild_only,
        default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
    )]
    pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
        use serenity::futures::StreamExt;
        ctx.defer().await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let mut settings = ctx
            .data()
            .service
            .voice_tracking
            .get_server_settings(guild_id)
            .await?;

        let msg_handle = ctx.send(VoiceCog::create_settings_reply(&settings)).await?;

        let msg = msg_handle.message().await?.into_owned();
        let author_id = ctx.author().id;

        let mut collector = ComponentInteractionCollector::new(ctx.serenity_context())
            .message_id(msg.id)
            .author_id(author_id)
            .timeout(Duration::from_secs(120))
            .stream();

        while let Some(interaction) = collector.next().await {
            let mut should_update = true;

            match &interaction.data.kind {
                ComponentInteractionDataKind::StringSelect { values }
                    if interaction.data.custom_id == "voice_settings_enabled" =>
                {
                    if let Some(value) = values.first() {
                        settings.voice_tracking_enabled = Some(value == "true");
                    }
                }
                _ => {
                    should_update = false;
                }
            }

            if should_update {
                ctx.data()
                    .service
                    .voice_tracking
                    .update_server_settings(guild_id, settings.clone())
                    .await?;
            }

            interaction
                .create_response(
                    ctx.http(),
                    poise::serenity_prelude::CreateInteractionResponse::UpdateMessage(
                        poise::serenity_prelude::CreateInteractionResponseMessage::new()
                            .components(VoiceCog::create_settings_components(&settings)),
                    ),
                )
                .await?;
        }

        Ok(())
    }

    #[poise::command(slash_command, guild_only)]
    pub async fn leaderboard(ctx: Context<'_>) -> Result<(), Error> {
        ctx.defer().await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let leaderboard = ctx
            .data()
            .service
            .voice_tracking
            .get_leaderboard(guild_id, 10)
            .await?;

        if leaderboard.is_empty() {
            ctx.say("No voice activity recorded yet.").await?;
            return Ok(());
        }

        let mut content = String::from("## 🎤 Voice Leaderboard\n\n");
        for (i, entry) in leaderboard.iter().enumerate() {
            let user_id = UserId::new(entry.user_id);
            let duration = Duration::from_secs(entry.total_duration as u64);
            let hours = duration.as_secs() / 3600;
            let minutes = (duration.as_secs() % 3600) / 60;

            content.push_str(&format!(
                "{}. <@{}>: **{}h {}m**\n",
                i + 1,
                user_id,
                hours,
                minutes
            ));
        }

        ctx.say(content).await?;
        Ok(())
    }

    #[poise::command(slash_command)]
    pub async fn history(ctx: Context<'_>) -> Result<(), Error> {
        ctx.say("🚧 This command is coming soon!").await?;
        Ok(())
    }

    fn create_settings_reply(settings: &ServerSettings) -> CreateReply<'_> {
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(VoiceCog::create_settings_components(settings))
    }

    fn create_settings_components(settings: &ServerSettings) -> Vec<CreateComponent<'_>> {
        let is_enabled = settings.voice_tracking_enabled.unwrap_or(true);

        // Status section
        let status_text = format!(
            "## Voice Tracking Settings\n\n> 🛈  {}",
            if is_enabled {
                "Voice tracking is **active**."
            } else {
                "Voice tracking is **paused**."
            }
        );
        let enabled_select = CreateSelectMenu::new(
            "voice_settings_enabled",
            CreateSelectMenuKind::String {
                options: vec![
                    CreateSelectMenuOption::new("🟢 Enabled", "true").default_selection(is_enabled),
                    CreateSelectMenuOption::new("🔴 Disabled", "false")
                        .default_selection(!is_enabled),
                ]
                .into(),
            },
        )
        .placeholder("Toggle voice tracking");

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(status_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(enabled_select)),
        ]));

        vec![container]
    }
}
