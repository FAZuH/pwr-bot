//! Voice stats subcommand.
use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use chrono::NaiveDate;
use contribution_grid::ContributionGraph;
use contribution_grid::builtins::Strategy;
use contribution_grid::builtins::Theme;
use log::trace;
use pwr_ext::component;

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

    Router::new(ctx)
        .run(Navigation::VoiceStats {
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

pub struct VoiceStatsView {
    pub model: VoiceStatsModel,
    pub data: VoiceStatsData,
    pub image_bytes: Option<Vec<u8>>,
    pub service: std::sync::Arc<dyn VoiceTracker>,
    pub guild_id: u64,
    pub user: User,
}

impl VoiceStatsView {
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
impl ViewHandler for VoiceStatsView {
    type Action = VoiceStatsAction;
    async fn handle(&mut self, ctx: ViewContext<'_, VoiceStatsAction>) -> Result<ViewCmd, Error> {
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
                    && let Ok(user) = user_id.to_user(ctx.poise.http()).await
                {
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

        Ok(ViewCmd::Render)
    }
}

impl ViewRender for VoiceStatsView {
    type Action = VoiceStatsAction;
    fn render(&self, registry: &mut ActionRegistry<VoiceStatsAction>) -> ResponseKind<'_> {
        use VoiceStatsAction::*;

        // Keep the original registration order so the custom_id counter
        // assignment (pinned by the GUI tests) stays byte-identical.
        let toggle = registry.register(ToggleDataMode);
        let time_yearly = registry.register(TimeYearly);
        let time_monthly = registry.register(TimeMonthly);
        let time_weekly = registry.register(TimeWeekly);
        let time_hourly = registry.register(TimeHourly);
        let stat_unique = if !self.model.is_user_stats() {
            Some(registry.register(StatUniqueUsers))
        } else {
            None
        };
        let stat_total = registry.register(StatTotalTime);
        let stat_avg = registry.register(StatAverageTime);
        let user_select = if self.model.is_user_stats() {
            Some(registry.register(SelectUser))
        } else {
            None
        };

        let toggle_label = if self.model.is_user_stats() {
            "Show server stats"
        } else {
            "Show user stats"
        };

        // Media gallery when there is data, otherwise a "no activity" notice.
        let focus_component = if self.data.user_activity.is_empty()
            && self.data.guild_stats.is_empty()
        {
            CreateContainerComponent::TextDisplay(component! {
                text_display {
                    content: "No voice activity recorded for this time range.\n\nJoin a **voice channel** to start tracking!"
                }
            })
        } else {
            CreateContainerComponent::MediaGallery(component! {
                media_gallery {
                    media_gallery_item {
                        media: format!("attachment://{VOICE_STATS_IMAGE_FILENAME}")
                    }
                }
            })
        };

        let container = CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(component! {
                text_display { content: self.format_stats_summary() }
            }),
            CreateContainerComponent::Separator(component! {
                separator { divider: true }
            }),
            focus_component,
            CreateContainerComponent::ActionRow(component! {
                action_row {
                    button {
                        custom_id: toggle.id,
                        label: toggle_label,
                        style: ButtonStyle::Primary
                    }
                }
            }),
        ]);

        let mut components = vec![CreateComponent::Container(container)];

        components.push(CreateComponent::ActionRow(component! {
            action_row {
                button {
                    custom_id: time_yearly.id,
                    label: time_yearly.label,
                    style: if self.model.time_range == VoiceStatsTimeRange::Yearly {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    }
                }
                button {
                    custom_id: time_monthly.id,
                    label: time_monthly.label,
                    style: if self.model.time_range == VoiceStatsTimeRange::Monthly {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    }
                }
                button {
                    custom_id: time_weekly.id,
                    label: time_weekly.label,
                    style: if self.model.time_range == VoiceStatsTimeRange::Weekly {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    }
                }
                button {
                    custom_id: time_hourly.id,
                    label: time_hourly.label,
                    style: if self.model.time_range == VoiceStatsTimeRange::Hourly {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    }
                }
            }
        }));

        // Aggregation buttons (Unique Users only for the guild view).
        let mut stat_buttons = vec![];
        if let Some(unique) = &stat_unique {
            stat_buttons.push(unique.clone().as_button().style(
                if self.model.stat_type == GuildStatType::ActiveUserCount {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Secondary
                },
            ));
        }
        stat_buttons.push(stat_total.clone().as_button().style(
            if self.model.stat_type == GuildStatType::TotalTime {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Secondary
            },
        ));
        stat_buttons.push(stat_avg.clone().as_button().style(
            if self.model.stat_type == GuildStatType::AverageTime {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Secondary
            },
        ));
        components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
            stat_buttons.into(),
        )));

        // User select menu (only for the user view).
        if self.model.is_user_stats() {
            let select = user_select.as_ref().expect("user select registered");
            let user_kind = CreateSelectMenuKind::User {
                default_users: Some(std::borrow::Cow::Owned(vec![self.user.id])),
            };
            components.push(CreateComponent::ActionRow(component! {
                action_row {
                    select_menu {
                        custom_id: select.id.clone(),
                        kind: user_kind
                    }
                }
            }));
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

/// Handler for voice stats display and interaction.
pub struct VoiceStatsHandler<'a> {
    #[allow(dead_code)]
    ctx: Context<'a>,
    pub time_range: VoiceStatsTimeRange,
    pub target_user: Option<User>,
    pub stat_type: GuildStatType,
}

impl<'a> VoiceStatsHandler<'a> {
    /// Creates a new stats handler.
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
impl CommandHandler for VoiceStatsHandler<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let start = Instant::now();

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

        let mut view = VoiceStatsView::new(
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

        trace!("stats_initial_response {} ms", start.elapsed().as_millis());

        engine.run().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::NaiveDate;

    use super::*;
    use crate::bot::command::voice::test_support::StubVoiceTracker;
    use crate::bot::view::ActionRegistry;
    use crate::bot::view::ResponseKind;

    /// Rewrites every `custom_id` of the shape `Type:timestamp:counter` to a
    /// stable sentinel `id:Type`, and redacts now-dependent Discord timestamps
    /// (`<t:digits:f>` → `<t:TS:f>`) from text content, so the rendered shape is
    /// reproducible across runs while still pinning kind/label/style/order.
    fn normalize(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::String(cid)) = map.get("custom_id") {
                    let parts: Vec<&str> = cid.split(':').collect();
                    if parts.len() == 3
                        && parts[1].chars().all(|c| c.is_ascii_digit())
                        && parts[2].chars().all(|c| c.is_ascii_digit())
                    {
                        map.insert(
                            "custom_id".to_string(),
                            serde_json::json!(format!("id:{}", parts[0])),
                        );
                    }
                }
                for v in map.values_mut() {
                    normalize(v);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    normalize(v);
                }
            }
            serde_json::Value::String(s) => {
                let redacted = redact_timestamps(s);
                if redacted != *s {
                    *s = redacted;
                }
            }
            _ => {}
        }
    }

    /// Redacts the numeric unix timestamp in a Discord `<t:...>` tag.
    fn redact_timestamps(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(idx) = rest.find("<t:") {
            let (before, after) = rest.split_at(idx);
            out.push_str(before);
            // after begins with "<t:...>"; find the closing '>'.
            let close = after.find('>').expect("unclosed <t: tag");
            let (tag, remaining) = after.split_at(close + 1);
            let parts: Vec<&str> = tag.split(':').collect();
            out.push_str("<t:TS");
            for p in &parts[2..] {
                out.push(':');
                out.push_str(p);
            }
            rest = remaining;
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn voice_stats_render_snapshot_guild_state() {
        let user: User = serde_json::from_value(serde_json::json!({
            "id": "123456789",
            "username": "tester"
        }))
        .unwrap();

        let data = VoiceStatsData {
            user: None,
            guild_name: "Test Server".to_string(),
            user_activity: vec![],
            guild_stats: vec![
                GuildDailyStats {
                    day: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                    value: 3600,
                },
                GuildDailyStats {
                    day: NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
                    value: 7200,
                },
            ],
            stat_type: GuildStatType::TotalTime,
            time_range: VoiceStatsTimeRange::Monthly,
            raw_sessions: vec![],
        };

        let view = VoiceStatsView {
            model: VoiceStatsModel {
                time_range: VoiceStatsTimeRange::Monthly,
                stat_type: GuildStatType::TotalTime,
                user_id: None,
                fallback_user_id: 1,
            },
            data,
            image_bytes: None,
            service: Arc::new(StubVoiceTracker),
            guild_id: 1,
            user,
        };

        let mut registry = ActionRegistry::new();
        let response = view.render(&mut registry);
        let ResponseKind::Component(components) = response else {
            panic!("expected a component response");
        };
        let mut value = serde_json::to_value(&components).unwrap();
        normalize(&mut value);
        assert_eq!(
            value,
            serde_json::json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "### Voice Stats\n-# Time Range: **Monthly** — <t:TS:f> to <t:TS:R>\n\n**Server:** Test Server\n**Peak Total Time:** 2h\n**Most Active:** 2h on <t:TS:d>"
                        },
                        { "type": 14, "divider": true },
                        {
                            "type": 12,
                            "items": [ { "media": { "url": "attachment://voice_stats.png" } } ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:VoiceStatsAction",
                                    "disabled": false,
                                    "label": "Show user stats",
                                    "style": 1
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Yearly", "style": 2 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Monthly", "style": 1 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Weekly", "style": 2 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Hourly", "style": 2 }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Unique Users", "style": 2 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Total Time", "style": 1 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Average Time", "style": 2 }
                    ]
                }
            ])
        );
    }

    #[test]
    fn voice_stats_render_snapshot_user_state() {
        let user: User = serde_json::from_value(serde_json::json!({
            "id": "123456789",
            "username": "tester"
        }))
        .unwrap();

        let data = VoiceStatsData {
            user: Some(user.clone()),
            guild_name: "Test Server".to_string(),
            user_activity: vec![],
            guild_stats: vec![],
            stat_type: GuildStatType::TotalTime,
            time_range: VoiceStatsTimeRange::Monthly,
            raw_sessions: vec![],
        };

        let view = VoiceStatsView {
            model: VoiceStatsModel {
                time_range: VoiceStatsTimeRange::Monthly,
                stat_type: GuildStatType::TotalTime,
                user_id: Some(user.id.get()),
                fallback_user_id: 1,
            },
            data,
            image_bytes: None,
            service: Arc::new(StubVoiceTracker),
            guild_id: 1,
            user,
        };

        let mut registry = ActionRegistry::new();
        let response = view.render(&mut registry);
        let ResponseKind::Component(components) = response else {
            panic!("expected a component response");
        };
        let mut value = serde_json::to_value(&components).unwrap();
        normalize(&mut value);
        assert_eq!(
            value,
            serde_json::json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "### Voice Stats\n-# Time Range: **Monthly** — <t:TS:f> to <t:TS:R>\n\n**User:** tester\n**Total Time:** 0s\n**Average Daily:** 0s\n**Current Streak:** 0 day(s)"
                        },
                        { "type": 14, "divider": true },
                        {
                            "type": 10,
                            "content": "No voice activity recorded for this time range.\n\nJoin a **voice channel** to start tracking!"
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:VoiceStatsAction",
                                    "disabled": false,
                                    "label": "Show server stats",
                                    "style": 1
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Yearly", "style": 2 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Monthly", "style": 1 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Weekly", "style": 2 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Hourly", "style": 2 }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Total Time", "style": 1 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Average Time", "style": 2 }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 5,
                            "custom_id": "id:VoiceStatsAction",
                            "default_values": [ { "id": 123456789, "type": "user" } ]
                        }
                    ]
                }
            ])
        );
    }
}
