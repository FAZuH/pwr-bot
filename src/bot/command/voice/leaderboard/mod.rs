//! Voice leaderboard subcommand.

use std::sync::Arc;
use std::time::Duration;

use crate::bot::command::prelude::*;
use crate::bot::command::voice::TimeRange;
use crate::bot::command::voice::VoiceLeaderboardTimeRange;
use crate::bot::command::voice::leaderboard::image_builder::LeaderboardImageBuilder;
use crate::bot::gui::Host;
use crate::bot::gui::voice_leaderboard::LEADERBOARD_PER_PAGE;
use crate::bot::gui::voice_leaderboard::VoiceLeaderboardConfig;
use crate::bot::gui::voice_leaderboard::VoiceLeaderboardEffectHandler;
use crate::bot::gui::voice_leaderboard::VoiceLeaderboardFeature;
use crate::entity::VoiceLeaderboardEntry;
use crate::entity::VoiceLeaderboardOptBuilder;
use crate::service::traits::VoiceTracker;
use crate::update::voice_leaderboard::VoiceLeaderboardModel;

pub mod image_builder;
pub mod image_generator;

/// Display the voice activity leaderboard
///
/// Shows a ranked list of users by total time spent in voice channels.
/// Includes your current rank position.
#[poise::command(slash_command)]
pub async fn leaderboard(
    ctx: Context<'_>,
    #[description = "Time period to filter voice activity. Defaults to \"This month\""]
    time_range: Option<VoiceLeaderboardTimeRange>,
) -> Result<(), Error> {
    Router::new(ctx)
        .run(Navigation::VoiceLeaderboard {
            time_range: time_range.unwrap_or(VoiceLeaderboardTimeRange::ThisMonth),
        })
        .await?;
    Ok(())
}

handler! { pub struct VoiceLeaderboardHandler<'a> {
    time_range: VoiceLeaderboardTimeRange,
} }

#[async_trait::async_trait]
impl CommandHandler for VoiceLeaderboardHandler<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
        let author_id = ctx.author().id.get();
        let http = ctx.serenity_context().http.clone();
        let service = ctx.data().service.voice_tracking.clone();

        let entries =
            fetch_entries(&service, guild_id, author_id, self.time_range, false, None).await?;

        let model =
            VoiceLeaderboardModel::from_entries(entries.clone(), author_id, LEADERBOARD_PER_PAGE);

        let image_bytes = if !model.is_empty() {
            let mut img_builder = LeaderboardImageBuilder::new(http.clone());
            let page_entries = model.current_page_entries();
            let rank_offset = model.current_page_rank_offset();
            img_builder
                .build(page_entries, rank_offset)
                .await
                .ok()
                .map(|res| res.image_bytes)
        } else {
            None
        };

        let config = VoiceLeaderboardConfig {
            entries,
            author_id,
            per_page: LEADERBOARD_PER_PAGE,
            image_bytes,
        };

        let handler = VoiceLeaderboardEffectHandler::new(service, guild_id, author_id, http);

        let mut host = Host::<VoiceLeaderboardFeature, _>::new(
            ctx,
            config,
            handler,
            Duration::from_mins(2),
            coordinator.clone(),
        );

        host.run().await?;

        Ok(())
    }
}

/// Fetches leaderboard entries for the given filters.
async fn fetch_entries(
    service: &Arc<dyn VoiceTracker>,
    guild_id: u64,
    author_id: u64,
    time_range: VoiceLeaderboardTimeRange,
    is_partner_mode: bool,
    target_user: Option<poise::serenity_prelude::UserId>,
) -> Result<Vec<VoiceLeaderboardEntry>, Error> {
    let (since, until) = time_range.to_range();

    let voice_lb_opts = VoiceLeaderboardOptBuilder::default()
        .guild_id(guild_id)
        .limit(Some(u32::MAX))
        .since(Some(since))
        .until(Some(until))
        .build()
        .map_err(AppError::from)?;

    let entries = if is_partner_mode {
        let target_id = target_user.unwrap_or_else(|| UserId::new(author_id)).get();
        service
            .get_partner_leaderboard(&voice_lb_opts, target_id)
            .await
            .map_err(Error::from)?
    } else {
        service
            .get_leaderboard_withopt(&voice_lb_opts)
            .await
            .map_err(Error::from)?
    };

    Ok(entries)
}
