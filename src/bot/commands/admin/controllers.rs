use poise::samples::create_application_commands;

use crate::bot::checks::is_author_guild_admin;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::admin::registration::CommandRegistrationView;
use crate::bot::commands::admin::registration::CommandUnregistrationView;
use crate::bot::commands::admin::views::SettingsMainAction;
use crate::bot::commands::admin::views::SettingsMainView;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::InteractableComponentView;
use crate::bot::views::ResponseProvider;
use crate::bot::views::StatefulView;
use crate::controller;
use crate::database::model::ServerSettingsModel;
use crate::database::table::Table;

controller! { pub struct SettingsMainController<'a> {} }

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for SettingsMainController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        is_author_guild_admin(ctx).await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;

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
        view.send().await?;

        while let Some((action, _)) = view.listen_once().await? {
            view.edit().await?;
            if view.is_settings_modified {
                ctx.data()
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

    // Send initial view
    let initial_view = CommandRegistrationView::new(num_commands);
    let msg = ctx.send(initial_view.create_reply()).await?;

    // Register commands
    guild_id.set_commands(ctx.http(), &create_commands).await?;

    // Update with completion view
    let duration_ms = start_time.elapsed().as_millis() as u64;
    let complete_view = CommandRegistrationView::new(num_commands).complete(duration_ms);
    msg.edit(ctx, complete_view.create_reply()).await?;

    Ok(())
}

/// Legacy function for unregistering guild commands.
pub async fn unregister(ctx: Context<'_>) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;

    let start_time = std::time::Instant::now();

    // Send initial view
    let initial_view = CommandUnregistrationView::new();
    let msg = ctx.send(initial_view.create_reply()).await?;

    // Unregister commands
    guild_id.set_commands(ctx.http(), &[]).await?;

    // Update with completion view
    let duration_ms = start_time.elapsed().as_millis() as u64;
    let complete_view = CommandUnregistrationView::new().complete(duration_ms);
    msg.edit(ctx, complete_view.create_reply()).await?;

    Ok(())
}

/// Entrypoint for /settings command
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    crate::bot::commands::settings::run_settings(ctx, None).await
}
