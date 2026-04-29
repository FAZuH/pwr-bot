//! Voice stats subcommand.
use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use chrono::NaiveDate;
use contribution_grid::ContributionGraph;
use contribution_grid::builtins::Strategy;
use contribution_grid::builtins::Theme;
use log::trace;

use crate::bot::command::prelude::*;
use crate::bot::command::voice::GuildStatType;
use crate::bot::command::voice::TimeRange;
use crate::bot::command::voice::VoiceStatsTimeRange;
use crate::bot::command::voice::stats::chart::generate_line_chart;
use crate::entity::GuildDailyStats;
use crate::entity::VoiceDailyActivity;
use crate::entity::VoiceSessionsEntity;
use crate::service::traits::VoiceTracker;
use crate::update::Update;
use crate::update::voice_stats::VoiceStatsCmd;
use crate::update::voice_stats::VoiceStatsModel;
use crate::update::voice_stats::VoiceStatsMsg;
use crate::update::voice_stats::VoiceStatsUpdate;

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

    Coordinator::new(ctx)
        .run(NavigationResult::VoiceStats {
            time_range,
            target_user: Box::new(target_user),
            stat_type,
        })
        .await?;
    Ok(())
}

/// Filename for the voice stats image attachment.
pub const VOICE_STATS_IMAGE_FILENAME: &str = "voice_stats.png";

action_enum! {
    VoiceStatsAction {
        #[label = "Yearly"]
        TimeYearly,
        #[label = "Monthly"]
        TimeMonthly,
        #[label = "Weekly"]
        TimeWeekly,
        #[label = "Hourly"]
        TimeHourly,

        #[label = "Unique Users"]
        StatUniqueUsers,
        #[label = "Total Time"]
        StatTotalTime,
        #[label = "Average Time"]
        StatAverageTime,

        ToggleDataMode,
        SelectUser,
    }
}

/// Data for voice stats display.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VoiceStatsData {
    /// The user this data is for (None for guild stats)
    pub user: Option<User>,
    /// The server name (for display purposes)
    pub guild_name: String,
    /// Daily activity data for users
    pub user_activity: Vec<VoiceDailyActivity>,
    /// Daily stats for guild (either average time or user count)
    pub guild_stats: Vec<GuildDailyStats>,
    /// Current stat type being displayed (for server view)
    pub stat_type: GuildStatType,
    /// Time range for the data
    pub time_range: VoiceStatsTimeRange,
    /// Raw sessions for line chart generation
    pub raw_sessions: Vec<VoiceSessionsEntity>,
}

impl VoiceStatsData {
    /// Returns true if this is showing user stats (not guild stats).
    pub fn is_user_stats(&self) -> bool {
        self.user.is_some()
    }

    /// Gets the display name for the stats subject.
    pub fn display_name(&self) -> String {
        match &self.user {
            Some(user) => user.name.to_string(),
            None => self.guild_name.clone(),
        }
    }

    /// Calculates total time from user activity data.
    pub fn total_time(&self) -> i64 {
        self.user_activity.iter().map(|a| a.total_seconds).sum()
    }

    /// Calculates average daily time.
    pub fn average_daily_time(&self) -> i64 {
        if self.user_activity.is_empty() {
            return 0;
        }
        self.total_time() / self.user_activity.len() as i64
    }

    /// Finds the most active day.
    pub fn most_active_day(&self) -> Option<(NaiveDate, i64)> {
        self.user_activity
            .iter()
            .max_by_key(|a| a.total_seconds)
            .map(|a| (a.day, a.total_seconds))
    }

    /// Calculates current streak (consecutive days with activity up to today).
    pub fn current_streak(&self) -> u32 {
        if self.user_activity.is_empty() {
            return 0;
        }

        let today = chrono::Local::now().date_naive();
        let mut streak = 0;

        // Sort by date descending
        let mut sorted: Vec<_> = self.user_activity.iter().map(|a| a.day).collect();
        sorted.sort_by(|a, b| b.cmp(a));

        // Check if today has activity
        if sorted.first() != Some(&today) {
            // Check if yesterday has activity (streak could be ongoing)
            let yesterday = today.pred_opt().unwrap_or(today);
            if sorted.first() != Some(&yesterday) {
                return 0;
            }
        }

        // Count consecutive days
        let mut expected = sorted[0];
        for day in sorted {
            if day == expected {
                streak += 1;
                expected = expected.pred_opt().unwrap_or(expected);
            } else {
                break;
            }
        }

        streak
    }

    /// Gets the maximum value for guild stats (for scaling).
    pub fn max_guild_stat_value(&self) -> i64 {
        self.guild_stats.iter().map(|s| s.value).max().unwrap_or(0)
    }

    /// Gets the total for guild user count stats.
    pub fn total_active_users(&self) -> i64 {
        self.guild_stats.iter().map(|s| s.value).sum()
    }
}

pub struct VoiceStatsHandler {
    pub model: VoiceStatsModel,
    pub data: VoiceStatsData,
    pub image_bytes: Option<Vec<u8>>,
    pub service: std::sync::Arc<dyn VoiceTracker>,
    pub guild_id: u64,
    pub user: User,
}

impl VoiceStatsHandler {
    pub(crate) fn new(
        data: VoiceStatsData,
        service: std::sync::Arc<dyn VoiceTracker>,
        guild_id: u64,
        user: User,
    ) -> Self {
        let model = VoiceStatsModel {
            time_range: data.time_range,
            stat_type: data.stat_type,
            user_id: data.user.as_ref().map(|u| u.id.get()),
            fallback_user_id: user.id.get(),
        };
        Self {
            model,
            data,
            image_bytes: None,
            service,
            guild_id,
            user,
        }
    }

    async fn refetch_data(&mut self) -> Result<(), Error> {
        let (since, until) = self.model.time_range.to_range();

        let raw_sessions = if self.model.time_range != VoiceStatsTimeRange::Yearly {
            self.service
                .get_sessions_in_range(self.guild_id, self.model.user_id, &since, &until)
                .await
                .map_err(Error::from)?
        } else {
            vec![]
        };

        if let Some(target_user_id) = self.model.user_id {
            let user_activity = self
                .service
                .get_user_daily_activity(target_user_id, self.guild_id, &since, &until)
                .await
                .map_err(Error::from)?;

            self.data = VoiceStatsData {
                user: Some(self.user.clone()),
                guild_name: self.data.guild_name.clone(),
                user_activity,
                guild_stats: vec![],
                stat_type: self.model.stat_type,
                time_range: self.model.time_range,
                raw_sessions,
            };
        } else {
            let guild_stats = self
                .service
                .get_guild_daily_stats(self.guild_id, &since, &until, self.model.stat_type)
                .await
                .map_err(Error::from)?;

            self.data = VoiceStatsData {
                user: None,
                guild_name: self.data.guild_name.clone(),
                user_activity: vec![],
                guild_stats,
                stat_type: self.model.stat_type,
                time_range: self.model.time_range,
                raw_sessions,
            };
        }

        if let Ok(bytes) = self.generate_image() {
            self.image_bytes = Some(bytes);
        } else {
            self.image_bytes = None;
        }

        Ok(())
    }

    /// Generates the contribution grid image.
    fn generate_image(&self) -> anyhow::Result<Vec<u8>> {
        if self.model.time_range != VoiceStatsTimeRange::Yearly {
            return generate_line_chart(
                &self.data.raw_sessions,
                self.model.time_range,
                self.model.stat_type,
                self.model.is_user_stats(),
            );
        }

        let (since, _until) = self.model.time_range.to_range();
        let today = chrono::Local::now().date_naive();

        // Build data map for contribution grid
        let mut data_map: HashMap<NaiveDate, u32> = HashMap::new();

        if self.model.is_user_stats() {
            // User activity: map day -> total seconds (converted to minutes for display)
            for activity in &self.data.user_activity {
                let minutes = (activity.total_seconds / 60).max(1) as u32;
                data_map.insert(activity.day, minutes);
            }
        } else {
            // Guild stats: map day -> value (minutes for time, count for users)
            for stat in &self.data.guild_stats {
                let value = if self.model.stat_type == GuildStatType::AverageTime
                    || self.model.stat_type == GuildStatType::TotalTime
                {
                    (stat.value / 60).max(1) as u32
                } else {
                    stat.value as u32
                };
                data_map.insert(stat.day, value);
            }
        }

        // Generate the graph with appropriate date range
        let img = ContributionGraph::new()
            .with_data(data_map)
            .start_date(since.date_naive())
            .end_date(today)
            .theme(Theme::github(Strategy::linear()))
            .generate();

        // Convert to PNG bytes
        let mut bytes: Vec<u8> = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )?;

        Ok(bytes)
    }

    /// Formats the stats summary text.
    fn format_stats_summary(&self) -> String {
        let (since, until) = self.model.time_range.to_range();
        let time_range_text = format!(
            "-# Time Range: **{}** — <t:{}:f> to <t:{}:R>",
            self.model.time_range.display_name(),
            since.timestamp(),
            until.timestamp(),
        );

        if self.model.is_user_stats() {
            let total = format_duration(self.data.total_time());
            let avg = format_duration(self.data.average_daily_time());
            let streak = self.data.current_streak();

            format!(
                "### Voice Stats\n{}\n\n**User:** {}\n**Total Time:** {}\n**Average Daily:** {}\n**Current Streak:** {} day(s)",
                time_range_text,
                self.data.display_name(),
                total,
                avg,
                streak
            )
        } else {
            // For guild stats, show different metrics based on stat_type
            let (first_label, first_value, second_label, second_value) = match self.model.stat_type
            {
                GuildStatType::AverageTime => {
                    // Peak Time: highest average voice time per user
                    let peak = self.data.guild_stats.iter().max_by_key(|s| s.value);
                    let peak_str = peak
                        .map(|s| format_duration(s.value))
                        .unwrap_or_else(|| "None".to_string());
                    let peak_day = peak
                        .map(|s| s.day)
                        .unwrap_or_else(|| chrono::Utc::now().date_naive());
                    let peak_day_str = format!(
                        " {} on <t:{}:d>",
                        peak_str,
                        peak_day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()
                    );

                    ("Peak Time", peak_str, "Most Active", peak_day_str)
                }
                GuildStatType::ActiveUserCount => {
                    // Avg Daily Users: average number of active users per day
                    let active_users = if self.data.guild_stats.is_empty() {
                        0
                    } else {
                        let total_days = self.data.guild_stats.len() as i64;
                        (self.data.total_active_users() as f64 / total_days as f64).ceil() as i64
                    };

                    // Most Active: day with most users
                    let peak = self.data.guild_stats.iter().max_by_key(|s| s.value);
                    let peak_str = peak
                        .map(|s| s.value.to_string())
                        .unwrap_or_else(|| "None".to_string());
                    let peak_day = peak
                        .map(|s| s.day)
                        .unwrap_or_else(|| chrono::Utc::now().date_naive());
                    let peak_day_str = format!(
                        " {} on <t:{}:d>",
                        peak_str,
                        peak_day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()
                    );

                    (
                        "Avg Daily Users",
                        active_users.to_string(),
                        "Most Active",
                        peak_day_str,
                    )
                }
                GuildStatType::TotalTime => {
                    // Peak Time: highest total voice time
                    let peak = self.data.guild_stats.iter().max_by_key(|s| s.value);
                    let peak_str = peak
                        .map(|s| format_duration(s.value))
                        .unwrap_or_else(|| "None".to_string());
                    let peak_day = peak
                        .map(|s| s.day)
                        .unwrap_or_else(|| chrono::Utc::now().date_naive());
                    let peak_day_str = format!(
                        " {} on <t:{}:d>",
                        peak_str,
                        peak_day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()
                    );

                    ("Peak Total Time", peak_str, "Most Active", peak_day_str)
                }
            };

            format!(
                "### Voice Stats\n{}\n\n**Server:** {}\n**{}:** {}\n**{}:**{}",
                time_range_text,
                self.data.guild_name,
                first_label,
                first_value,
                second_label,
                second_value
            )
        }
    }
}

#[async_trait::async_trait]
impl ViewHandler for VoiceStatsHandler {
    type Action = VoiceStatsAction;
    async fn handle(
        &mut self,
        ctx: ViewContext<'_, VoiceStatsAction>,
    ) -> Result<ViewCommand, Error> {
        use VoiceStatsAction::*;

        let mut changed = false;

        match ctx.action() {
            TimeYearly => {
                let cmd = VoiceStatsUpdate::update(
                    VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Yearly),
                    &mut self.model,
                );
                changed = matches!(cmd, VoiceStatsCmd::RefetchData);
            }
            TimeMonthly => {
                let cmd = VoiceStatsUpdate::update(
                    VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Monthly),
                    &mut self.model,
                );
                changed = matches!(cmd, VoiceStatsCmd::RefetchData);
            }
            TimeWeekly => {
                let cmd = VoiceStatsUpdate::update(
                    VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Weekly),
                    &mut self.model,
                );
                changed = matches!(cmd, VoiceStatsCmd::RefetchData);
            }
            TimeHourly => {
                let cmd = VoiceStatsUpdate::update(
                    VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Hourly),
                    &mut self.model,
                );
                changed = matches!(cmd, VoiceStatsCmd::RefetchData);
            }

            StatUniqueUsers => {
                let cmd = VoiceStatsUpdate::update(
                    VoiceStatsMsg::ChangeStatType(GuildStatType::ActiveUserCount),
                    &mut self.model,
                );
                changed = matches!(cmd, VoiceStatsCmd::RefetchData);
            }
            StatTotalTime => {
                let cmd = VoiceStatsUpdate::update(
                    VoiceStatsMsg::ChangeStatType(GuildStatType::TotalTime),
                    &mut self.model,
                );
                changed = matches!(cmd, VoiceStatsCmd::RefetchData);
            }
            StatAverageTime => {
                let cmd = VoiceStatsUpdate::update(
                    VoiceStatsMsg::ChangeStatType(GuildStatType::AverageTime),
                    &mut self.model,
                );
                changed = matches!(cmd, VoiceStatsCmd::RefetchData);
            }

            ToggleDataMode => {
                let cmd = VoiceStatsUpdate::update(VoiceStatsMsg::ToggleDataMode, &mut self.model);
                changed = matches!(cmd, VoiceStatsCmd::RefetchData);
            }
            SelectUser => {
                if let Some(user_id) = ctx.user_select_values().and_then(|v| v.first().copied())
                    && let Ok(user) = user_id.to_user(ctx.poise.http()).await {
                        self.user = user.clone();
                        let cmd = VoiceStatsUpdate::update(
                            VoiceStatsMsg::SetUser(Some(user.id.get())),
                            &mut self.model,
                        );
                        changed = matches!(cmd, VoiceStatsCmd::RefetchData);
                    }
            }
        }

        if changed {
            self.refetch_data().await?;
        }

        Ok(ViewCommand::Render)
    }
}

impl ViewRender for VoiceStatsHandler {
    type Action = VoiceStatsAction;
    fn render(&self, registry: &mut ActionRegistry<VoiceStatsAction>) -> ResponseKind<'_> {
        use VoiceStatsAction::*;

        let mut container_components = vec![CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(self.format_stats_summary()),
        )];

        container_components.push(CreateContainerComponent::Separator(CreateSeparator::new(
            true,
        )));

        if self.data.user_activity.is_empty() && self.data.guild_stats.is_empty() {
            container_components.push(CreateContainerComponent::TextDisplay(
                CreateTextDisplay::new(
                    "No voice activity recorded for this time range.\n\nJoin a **voice channel** to start tracking!",
                ),
            ));
        } else {
            container_components.push(CreateContainerComponent::MediaGallery(
                CreateMediaGallery::new(vec![CreateMediaGalleryItem::new(
                    CreateUnfurledMediaItem::new(format!(
                        "attachment://{}",
                        VOICE_STATS_IMAGE_FILENAME
                    )),
                )]),
            ));
        }

        // Add Data Mode Toggle to bottom of Container
        let toggle_label = if self.model.is_user_stats() {
            "Show server stats"
        } else {
            "Show user stats"
        };

        let toggle_button = registry
            .register(ToggleDataMode)
            .as_button()
            .label(toggle_label)
            .style(ButtonStyle::Primary);

        container_components.push(CreateContainerComponent::ActionRow(
            CreateActionRow::Buttons(vec![toggle_button].into()),
        ));

        let mut components = vec![CreateComponent::Container(CreateContainer::new(
            container_components,
        ))];

        // 1. Time Range Row
        let time_buttons = vec![
            registry.register(TimeYearly).as_button().style(
                if self.model.time_range == VoiceStatsTimeRange::Yearly {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Secondary
                },
            ),
            registry.register(TimeMonthly).as_button().style(
                if self.model.time_range == VoiceStatsTimeRange::Monthly {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Secondary
                },
            ),
            registry.register(TimeWeekly).as_button().style(
                if self.model.time_range == VoiceStatsTimeRange::Weekly {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Secondary
                },
            ),
            registry.register(TimeHourly).as_button().style(
                if self.model.time_range == VoiceStatsTimeRange::Hourly {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Secondary
                },
            ),
        ];
        components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
            time_buttons.into(),
        )));

        // 2. Aggregation Row (Only for Guild)
        let mut stat_buttons = vec![];
        if !self.model.is_user_stats() {
            stat_buttons.push(registry.register(StatUniqueUsers).as_button().style(
                if self.model.stat_type == GuildStatType::ActiveUserCount {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Secondary
                },
            ));
        }
        stat_buttons.push(registry.register(StatTotalTime).as_button().style(
            if self.model.stat_type == GuildStatType::TotalTime {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Secondary
            },
        ));
        stat_buttons.push(registry.register(StatAverageTime).as_button().style(
            if self.model.stat_type == GuildStatType::AverageTime {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Secondary
            },
        ));
        components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
            stat_buttons.into(),
        )));

        // 3. User Select Menu (Only for User)
        if self.model.is_user_stats() {
            let default_users = Some(std::borrow::Cow::Owned(vec![self.user.id]));
            let user_select = registry
                .register(SelectUser)
                .as_select(CreateSelectMenuKind::User { default_users });
            components.push(CreateComponent::ActionRow(CreateActionRow::SelectMenu(
                user_select,
            )));
        }

        components.into()
    }

    fn create_reply(
        &self,
        registry: &mut ActionRegistry<VoiceStatsAction>,
    ) -> poise::CreateReply<'_> {
        let response = self.render(registry);
        let mut reply: poise::CreateReply<'_> = response.into();

        if let Some(ref bytes) = self.image_bytes {
            let attachment = CreateAttachment::bytes(bytes.clone(), VOICE_STATS_IMAGE_FILENAME);
            reply = reply.attachment(attachment);
        }

        reply
    }
}

/// Controller for voice stats display and interaction.
pub struct VoiceStatsController<'a> {
    #[allow(dead_code)]
    ctx: Context<'a>,
    pub time_range: VoiceStatsTimeRange,
    pub target_user: Option<User>,
    pub stat_type: GuildStatType,
}

impl<'a> VoiceStatsController<'a> {
    /// Creates a new stats controller.
    pub fn new(
        ctx: Context<'a>,
        time_range: VoiceStatsTimeRange,
        target_user: Option<User>,
        stat_type: GuildStatType,
    ) -> Self {
        Self {
            ctx,
            time_range,
            target_user,
            stat_type,
        }
    }

    /// Fetches stats data based on current parameters.
    async fn fetch_data(&self, ctx: &Context<'_>) -> Result<VoiceStatsData, Error> {
        let service = ctx.data().service.voice_tracking.clone();
        let (since, until) = self.time_range.to_range();

        // Get guild info
        let guild_name = if let Some(guild) = ctx.guild() {
            guild.name.to_string()
        } else {
            "Direct Messages".to_string()
        };

        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let raw_sessions = if self.time_range != VoiceStatsTimeRange::Yearly {
            service
                .get_sessions_in_range(
                    guild_id,
                    self.target_user.as_ref().map(|u| u.id.get()),
                    &since,
                    &until,
                )
                .await
                .map_err(Error::from)?
        } else {
            vec![]
        };

        if let Some(ref target_user) = self.target_user {
            // Fetch user-specific stats
            let user_activity = service
                .get_user_daily_activity(target_user.id.get(), guild_id, &since, &until)
                .await
                .map_err(Error::from)?;

            Ok(VoiceStatsData {
                user: Some(target_user.clone()),
                guild_name,
                user_activity,
                guild_stats: vec![],
                stat_type: self.stat_type,
                time_range: self.time_range,
                raw_sessions,
            })
        } else {
            // Fetch guild-wide stats
            let guild_stats = service
                .get_guild_daily_stats(guild_id, &since, &until, self.stat_type)
                .await
                .map_err(Error::from)?;

            Ok(VoiceStatsData {
                user: None,
                guild_name,
                user_activity: vec![],
                guild_stats,
                stat_type: self.stat_type,
                time_range: self.time_range,
                raw_sessions,
            })
        }
    }
}

#[async_trait::async_trait]
impl Controller for VoiceStatsController<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Coordinator<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let controller_start = Instant::now();

        // Fetch initial data
        let data = self.fetch_data(&ctx).await?;
        let guild_id = ctx
            .guild_id()
            .map(|id| id.get())
            .ok_or(BotError::GuildOnlyCommand)?;

        let user = self
            .target_user
            .clone()
            .unwrap_or_else(|| ctx.author().clone());

        let mut view = VoiceStatsHandler::new(
            data,
            ctx.data().service.voice_tracking.clone(),
            guild_id,
            user,
        );

        // Generate and send the image
        if !view.data.user_activity.is_empty()
            || !view.data.guild_stats.is_empty()
            || !view.data.raw_sessions.is_empty()
        {
            let bytes = view.generate_image().map_err(AppError::internal_with_ref)?;
            view.image_bytes = Some(bytes);
        }

        let mut engine = ViewEngine::new(ctx, view, Duration::from_secs(120), coordinator.clone());

        trace!(
            "stats_controller_initial_response {} ms",
            controller_start.elapsed().as_millis()
        );

        engine.run().await?;

        Ok(())
    }
}
