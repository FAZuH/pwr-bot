
use poise::CreateReply;
use poise::samples::create_application_commands;

use crate::bot::checks::is_author_guild_admin;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::admin::views::SettingsMainAction;
use crate::bot::commands::admin::views::SettingsMainView;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::InteractableComponentView;
use crate::bot::views::ResponseComponentView;
use crate::database::model::ServerSettingsModel;
use crate::database::table::Table;

/// Controller for admin settings with navigation support.
pub struct SettingsMainController<'a> {
    ctx: &'a Context<'a>,
}

impl<'a> SettingsMainController<'a> {
    /// Creates a new admin settings controller.
    pub fn new(ctx: &'a Context<'a>) -> Self {
        Self { ctx }
    }
}

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for SettingsMainController<'a> {
    async fn run(&mut self, coordinator: &mut Coordinator<'_, S>) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        is_author_guild_admin(ctx).await?;
        let guild_id = ctx
            .guild_id()
            .ok_or(BotError::GuildOnlyCommand)?;

        let settings = ctx
            .data()
            .db
            .server_settings_table
            .select(&guild_id.into())
            .await?
            .unwrap_or(ServerSettingsModel {
                guild_id: guild_id.into(),
                ..Default::default()
            });

        let mut view = SettingsMainView::new(&ctx, settings);
        coordinator.send(view.create_reply()).await?;

        while let Some((action, _)) = view.listen_once().await {
            coordinator.edit(view.create_reply()).await?;
            if view.is_settings_modified {
                ctx
                    .data()
                    .db
                    .server_settings_table
                    .replace(&view.settings)
                    .await?;
                view.done_update_settings()?;
            }

            // Check for navigation actions
            match action {
                SettingsMainAction::Feeds => {
                    return Ok(NavigationResult::SettingsFeeds);
                }
                SettingsMainAction::Voice => {
                    return Ok(NavigationResult::SettingsVoice);
                }
                SettingsMainAction::About => {
                    return Ok(NavigationResult::SettingsAbout);
                }
                _ => {}
            }
        }

        Ok(NavigationResult::Exit)
    }
}

/// Legacy function for registering guild commands.
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;

    let create_commands = create_application_commands(&ctx.framework().options().commands);
    let num_commands = create_commands.len();

    let start_time = std::time::Instant::now();
    let reply = ctx
        .reply(format!(
            ":gear: Registering {num_commands} guild commands..."
        ))
        .await?;
    guild_id.set_commands(ctx.http(), &create_commands).await?;

    reply
        .edit(
            ctx,
            CreateReply::default().content(format!(
                ":white_check_mark: Done! Took {}ms",
                start_time.elapsed().as_millis()
            )),
        )
        .await?;

    Ok(())
}

/// Legacy function for unregistering guild commands.
pub async fn unregister(ctx: Context<'_>) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;

    let start_time = std::time::Instant::now();
    let reply = ctx.reply(":gear: Unregistering guild commands...").await?;
    guild_id.set_commands(ctx.http(), &[]).await?;

    reply
        .edit(
            ctx,
            CreateReply::default().content(format!(
                ":white_check_mark: Done! Took {}ms",
                start_time.elapsed().as_millis()
            )),
        )
        .await?;

    Ok(())
}

/// Legacy function for admin settings command.
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    crate::bot::commands::settings::run_settings(ctx).await
}
