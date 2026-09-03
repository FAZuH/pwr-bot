//! Voice stats subcommand.

use std::sync::Arc;
use std::time::Duration;

use crate::bot::command::prelude::*;
use crate::bot::command::voice::GuildStatType;
use crate::bot::command::voice::TimeRange;
use crate::bot::command::voice::VoiceStatsTimeRange;
use crate::bot::gui::Host;
use crate::bot::gui::voice_stats::VoiceStatsConfig;
use crate::bot::gui::voice_stats::VoiceStatsEffectHandler;
use crate::bot::gui::voice_stats::VoiceStatsFeature;
use crate::bot::gui::voice_stats::generate_image;
use crate::service::traits::VoiceTracker;
use crate::update::voice_stats::VoiceStatsData;

pub mod chart;

/// Show voice activity statistics
///
/// Display daily voice activity for a user or the entire server.
#[poise::command(slash_command)]
pub async fn stats(
    ctx: Context<'_>,
    #[description = "Time period to display. Defaults to \"This month\""] time_range: Option<
        VoiceStatsTimeRange,
    >,
    #[description = "User to show stats for (defaults to server stats in server, yourself in DM)"]
    user: Option<poise::serenity_prelude::User>,
    #[description = "Statistic to display for server view"] statistic: Option<GuildStatType>,
) -> Result<(), Error> {
    command(ctx, time_range, user, statistic).await
}

/// Entry point for the stats command.
pub async fn command(
    ctx: Context<'_>,
    time_range: Option<VoiceStatsTimeRange>,
    user: Option<User>,
    statistic: Option<GuildStatType>,
) -> Result<(), Error> {
    let time_range = time_range.unwrap_or(VoiceStatsTimeRange::Monthly);
    let stat_type = statistic.unwrap_or_default();

    let target_user = if let Some(_guild_id) = ctx.guild_id() {
        if let Some(ref target) = user {
            if target.id != ctx.author().id {
                let is_member = ctx
                    .guild()
                    .map(|guild| guild.members.contains_key(&target.id))
                    .unwrap_or(false);

                if !is_member {
                    return Err(BotError::UserNotInGuild(
                        "The specified user is not a member of this server.".to_string(),
                    )
                    .into());
                }
            }
            Some(target.clone())
        } else {
            None
        }
    } else if let Some(ref target) = user {
        if target.id != ctx.author().id {
            return Err(BotError::UserNotInGuild(
                "In direct messages, you can only view your own voice stats.".to_string(),
            )
            .into());
        }
        Some(target.clone())
    } else {
        Some(ctx.author().clone())
    };

    Router::new(ctx)
        .run(Navigation::VoiceStats {
            time_range,
            target_user: Box::new(target_user),
            stat_type,
        })
        .await?;
    Ok(())
}

handler! { pub struct VoiceStatsHandler<'a> {
    time_range: VoiceStatsTimeRange,
    target_user: Option<User>,
    stat_type: GuildStatType,
} }

#[async_trait::async_trait]
impl CommandHandler for VoiceStatsHandler<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
        let guild_name = if let Some(guild) = ctx.guild() {
            guild.name.to_string()
        } else {
            "Direct Messages".to_string()
        };

        let author_id = ctx.author().id.get();
        let user_id = self.target_user.as_ref().map(|u| u.id.get());
        let fallback_user_id = author_id;

        let service = ctx.data().service.voice_tracking.clone();

        let data = fetch_initial_data(
            &service,
            guild_id,
            guild_name.clone(),
            self.target_user.as_ref(),
            self.time_range,
            self.stat_type,
        )
        .await?;

        let image_bytes = if !data.user_activity.is_empty()
            || !data.guild_stats.is_empty()
            || !data.raw_sessions.is_empty()
        {
            let bytes = generate_image(
                self.time_range,
                self.stat_type,
                self.target_user.is_some(),
                &data.raw_sessions,
                &data.user_activity,
                &data.guild_stats,
            )
            .map_err(AppError::internal_with_ref)?;
            Some(bytes)
        } else {
            None
        };

        let config = VoiceStatsConfig {
            time_range: self.time_range,
            stat_type: self.stat_type,
            user_id,
            fallback_user_id,
            data,
            image_bytes,
        };

        let handler = VoiceStatsEffectHandler::new(
            service.clone(),
            guild_id,
            guild_name,
            ctx.serenity_context().http.clone(),
        );

        let mut host = Host::<VoiceStatsFeature, _>::new(
            ctx,
            config,
            handler,
            Duration::from_secs(120),
            coordinator.clone(),
        );

        host.run().await?;

        Ok(())
    }
}

/// Fetches the initial stats snapshot, propagating database errors.
async fn fetch_initial_data(
    service: &Arc<dyn VoiceTracker>,
    guild_id: u64,
    guild_name: String,
    target_user: Option<&User>,
    time_range: VoiceStatsTimeRange,
    stat_type: GuildStatType,
) -> Result<VoiceStatsData, Error> {
    let (since, until) = time_range.to_range();

    let raw_sessions = if time_range != VoiceStatsTimeRange::Yearly {
        service
            .get_sessions_in_range(guild_id, target_user.map(|u| u.id.get()), &since, &until)
            .await
            .map_err(Error::from)?
    } else {
        vec![]
    };

    if let Some(target_user) = target_user {
        let user_activity = service
            .get_user_daily_activity(target_user.id.get(), guild_id, &since, &until)
            .await
            .map_err(Error::from)?;

        Ok(VoiceStatsData {
            guild_name,
            user_activity,
            guild_stats: vec![],
            raw_sessions,
            target_user_name: Some(target_user.name.to_string()),
        })
    } else {
        let guild_stats = service
            .get_guild_daily_stats(guild_id, &since, &until, stat_type)
            .await
            .map_err(Error::from)?;

        Ok(VoiceStatsData {
            guild_name,
            user_activity: vec![],
            guild_stats,
            raw_sessions,
            target_user_name: None,
        })
    }
}
