//! Voice command implementations.

use poise::CreateReply;
use serenity::all::CreateAttachment;
use serenity::all::MessageFlags;

use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::settings::SettingsPage;
use crate::bot::commands::settings::run_settings;
use crate::bot::commands::voice::LeaderboardEntry;
use crate::bot::commands::voice::VoiceLeaderboardTimeRange;
use crate::bot::commands::voice::image_generator::LeaderboardImageGenerator;
use crate::bot::commands::voice::views::SettingsVoiceAction;
use crate::bot::commands::voice::views::SettingsVoiceView;
use crate::bot::commands::voice::views::VoiceLeaderboardView;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::InteractableComponentView;
use crate::bot::views::ResponseComponentView;
use crate::bot::views::pagination::PaginationView;
use crate::controller;
use crate::database::model::VoiceLeaderboardEntry;
use crate::database::model::VoiceLeaderboardOptBuilder;
use crate::error::AppError;

/// Number of leaderboard entries per page.
const LEADERBOARD_PER_PAGE: u32 = 10;

controller! { pub struct VoiceSettingsController<'a> {} }

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for VoiceSettingsController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let settings = ctx
            .data()
            .service
            .voice_tracking
            .get_server_settings(guild_id)
            .await
            .map_err(Error::from)?;

        let mut view = SettingsVoiceView::new(&ctx, settings);
        coordinator.send(view.create_reply()).await?;

        while let Some((action, _interaction)) = view.listen_once().await {
            match action {
                SettingsVoiceAction::Back => return Ok(NavigationResult::Back),
                SettingsVoiceAction::About => {
                    return Ok(NavigationResult::SettingsAbout);
                }
                SettingsVoiceAction::ToggleEnabled => {
                    // Update the settings in the database
                    ctx.data()
                        .service
                        .voice_tracking
                        .update_server_settings(guild_id, view.settings.clone())
                        .await
                        .map_err(Error::from)?;

                    coordinator.edit(view.create_reply()).await?;
                }
            }
        }

        Ok(NavigationResult::Exit)
    }
}

controller! { pub struct VoiceLeaderboardController<'a> {
    time_range: VoiceLeaderboardTimeRange
} }

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for VoiceLeaderboardController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
        let author_id = ctx.author().id.get();

        let (since, until) = self.time_range.to_range();

        let voice_lb_otps = VoiceLeaderboardOptBuilder::default()
            .guild_id(guild_id)
            // TODO: Performance should be improved or this will become a problem
            .limit(Some(u32::MAX))
            .since(Some(since))
            .until(Some(until))
            .build()
            .map_err(AppError::from)?;

        let total_entries = ctx
            .data()
            .service
            .voice_tracking
            .get_leaderboard_withopt(&voice_lb_otps)
            .await
            .map_err(Error::from)?;

        if total_entries.is_empty() {
            let reply = VoiceLeaderboardView::create_empty_reply();
            ctx.send(reply).await?;
            return Ok(NavigationResult::Exit);
        }

        let total_items = total_entries.len() as u32;
        let mut pagination = PaginationView::new(&ctx, total_items, LEADERBOARD_PER_PAGE);

        let user_rank = total_entries
            .iter()
            .position(|e| e.user_id == author_id)
            .map(|pos| pos as u32 + 1);

        let image_gen = LeaderboardImageGenerator::new().map_err(|e| {
            AppError::internal_with_ref(format!("Failed to initialize image generator: {e}"))
        })?;

        let current_page_entries =
            &total_entries[..(total_entries.len().min(LEADERBOARD_PER_PAGE as usize))];
        let page_result = generate_page(ctx, &image_gen, current_page_entries, 0).await?;

        let view = VoiceLeaderboardView::new(user_rank);
        let mut components = view.create_components();
        pagination.attach_if_multipage(&mut components);
        let attachment = CreateAttachment::bytes(page_result.image_bytes, "voice_leaderboard.png");

        let reply = CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(components)
            .attachment(attachment);

        let msg_handle = ctx.send(reply).await?;

        while pagination.listen_once().await.is_some() {
            let current_page = pagination.state.current_page;
            let offset = ((current_page - 1) * LEADERBOARD_PER_PAGE) as usize;
            let end = (offset + LEADERBOARD_PER_PAGE as usize).min(total_entries.len());

            let page_entries = &total_entries[offset..end];
            let page_result = generate_page(ctx, &image_gen, page_entries, offset as u32).await?;

            components = view.create_components();
            pagination.attach_if_multipage(&mut components);

            let attachment = CreateAttachment::bytes(page_result.image_bytes, "leaderboard.png");

            let reply = CreateReply::new()
                .flags(MessageFlags::IS_COMPONENTS_V2)
                .components(components)
                .attachment(attachment);

            msg_handle.edit(ctx, reply).await?;
        }

        Ok(NavigationResult::Exit)
    }
}

/// Legacy function for voice settings command.
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    run_settings(ctx, Some(SettingsPage::Voice)).await
}

/// Legacy function for voice leaderboard command.
pub async fn leaderboard(
    ctx: Context<'_>,
    time_range: VoiceLeaderboardTimeRange,
) -> Result<(), Error> {
    let mut coordinator = Coordinator::new(ctx);
    let mut controller = VoiceLeaderboardController::new(&ctx, time_range);
    let _result = controller.run(&mut coordinator).await?;
    Ok(())
}

/// Generates a single page of the leaderboard.
async fn generate_page(
    ctx: Context<'_>,
    image_gen: &LeaderboardImageGenerator,
    entries: &[VoiceLeaderboardEntry],
    rank_offset: u32,
) -> Result<super::PageGenerationResult, Error> {
    let mut entries_with_names: Vec<(VoiceLeaderboardEntry, String)> = Vec::new();
    let mut entries_for_image: Vec<LeaderboardEntry> = Vec::new();

    for (idx, entry) in entries.iter().enumerate() {
        let rank = rank_offset + idx as u32 + 1;

        let (display_name, avatar_url) = match ctx.http().get_user(entry.user_id.into()).await {
            Ok(user) => {
                let name = user.name.to_string();
                let avatar = user.avatar_url();
                (name, avatar)
            }
            Err(_) => (format!("User {}", entry.user_id), None),
        };

        entries_with_names.push((entry.clone(), display_name.clone()));
        entries_for_image.push(LeaderboardEntry {
            rank,
            user_id: entry.user_id,
            display_name,
            avatar_url,
            duration_seconds: entry.total_duration,
        });
    }

    let image_bytes = image_gen
        .generate_leaderboard(&entries_for_image)
        .await
        .map_err(|e| {
            AppError::internal_with_ref(format!("Failed to initialize image generator: {}", e))
        })?;

    Ok(super::PageGenerationResult {
        entries_with_names,
        image_bytes,
    })
}
