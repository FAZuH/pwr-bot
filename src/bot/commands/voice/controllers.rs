//! Voice command implementations.

use std::ops::Deref;
use std::time::Instant;

use log::trace;
use serenity::all::CreateAttachment;

use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::settings::SettingsPage;
use crate::bot::commands::settings::run_settings;
use crate::bot::commands::voice::VoiceLeaderboardTimeRange;
use crate::bot::commands::voice::views::SettingsVoiceAction;
use crate::bot::commands::voice::views::SettingsVoiceView;
use crate::bot::commands::voice::views::VOICE_LEADERBOARD_IMAGE_FILENAME;
use crate::bot::commands::voice::views::VoiceLeaderboardAction;
use crate::bot::commands::voice::views::VoiceLeaderboardView;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::InteractableComponentView;
use crate::bot::views::ResponseComponentView;
use crate::controller;
use crate::database::model::VoiceLeaderboardEntry;
use crate::database::model::VoiceLeaderboardOptBuilder;
use crate::error::AppError;

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

        while let Some((action, _interaction)) = view.listen_once().await? {
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

/// Data for a leaderboard session.
pub struct LeaderboardSessionData {
    pub entries: Vec<VoiceLeaderboardEntry>,
    pub user_rank: Option<u32>,
    pub user_duration: Option<i64>,
}

impl LeaderboardSessionData {
    /// Creates session data from entries and calculates user rank.
    pub fn from_entries(entries: Vec<VoiceLeaderboardEntry>, author_id: u64) -> Self {
        let user_rank = entries
            .iter()
            .position(|e| e.user_id == author_id)
            .map(|p| p as u32 + 1);
        let user_duration = entries
            .iter()
            .find(|e| e.user_id == author_id)
            .map(|e| e.total_duration);

        Self {
            entries,
            user_rank,
            user_duration,
        }
    }
}

impl Deref for LeaderboardSessionData {
    type Target = Vec<VoiceLeaderboardEntry>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

/// Controller for voice leaderboard display and interaction.
pub struct VoiceLeaderboardController<'a> {
    #[allow(dead_code)]
    ctx: &'a Context<'a>,
    pub time_range: VoiceLeaderboardTimeRange,
}

impl<'a> VoiceLeaderboardController<'a> {
    /// Creates a new leaderboard controller.
    pub fn new(ctx: &'a Context<'a>, time_range: VoiceLeaderboardTimeRange) -> Self {
        Self { ctx, time_range }
    }

    /// Fetches leaderboard entries for the current time range.
    async fn fetch_entries(
        ctx: &Context<'_>,
        time_range: VoiceLeaderboardTimeRange,
    ) -> Result<LeaderboardSessionData, Error> {
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
        let (since, until) = time_range.to_range();

        let voice_lb_opts = VoiceLeaderboardOptBuilder::default()
            .guild_id(guild_id)
            .limit(Some(u32::MAX))
            .since(Some(since))
            .until(Some(until))
            .build()
            .map_err(AppError::from)?;

        let new_entries = ctx
            .data()
            .service
            .voice_tracking
            .get_leaderboard_withopt(&voice_lb_opts)
            .await
            .map_err(Error::from)?;

        let author_id = ctx.author().id.get();
        Ok(LeaderboardSessionData::from_entries(new_entries, author_id))
    }
}

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for VoiceLeaderboardController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let controller_start = Instant::now();

        let ctx = *coordinator.context();
        ctx.defer().await?;

        // Fetch initial entries
        let session_data = Self::fetch_entries(&ctx, self.time_range).await?;

        let mut view = VoiceLeaderboardView::new(&ctx, session_data, self.time_range);

        if view.leaderboard_data.is_empty() {
            let reply = view.create_reply();
            ctx.send(reply).await?;
            return Ok(NavigationResult::Exit);
        }

        // Generate and send initial page
        let page_result = view.generate_current_page().await?;
        let attachment =
            CreateAttachment::bytes(page_result.image_bytes, VOICE_LEADERBOARD_IMAGE_FILENAME);
        let reply = view.create_reply().attachment(attachment);
        coordinator.send(reply).await?;

        trace!(
            "controller_initial_response {} ms",
            controller_start.elapsed().as_millis()
        );

        while let Some((action, _)) = view.listen_once().await? {
            if matches!(action, VoiceLeaderboardAction::TimeRange) {
                let new_data = Self::fetch_entries(&ctx, view.time_range).await?;
                view.update_leaderboard_data(new_data);
            }
            let page_result = view.generate_current_page().await?;
            let attachment =
                CreateAttachment::bytes(page_result.image_bytes, VOICE_LEADERBOARD_IMAGE_FILENAME);
            let reply = view.create_reply().attachment(attachment);
            coordinator.edit(reply).await?;
        }

        trace!(
            "controller_total {} ms",
            controller_start.elapsed().as_millis()
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leaderboard_session_data_from_entries() {
        let entries = vec![
            VoiceLeaderboardEntry {
                user_id: 100,
                total_duration: 3600,
            },
            VoiceLeaderboardEntry {
                user_id: 200,
                total_duration: 1800,
            },
            VoiceLeaderboardEntry {
                user_id: 300,
                total_duration: 900,
            },
        ];

        // Test author is ranked #2
        let session = LeaderboardSessionData::from_entries(entries.clone(), 200);
        assert_eq!(session.user_rank, Some(2));
        assert_eq!(session.user_duration, Some(1800));

        // Test author not in list
        let session = LeaderboardSessionData::from_entries(entries, 999);
        assert_eq!(session.user_rank, None);
        assert_eq!(session.user_duration, None);
    }
}
