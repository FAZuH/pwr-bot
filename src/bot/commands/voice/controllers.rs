//! Voice command implementations.

use std::collections::HashMap;
use std::ops::Deref;
use std::time::Duration;
use std::time::Instant;

use chrono::NaiveDate;
use contribution_grid::ContributionGraph;
use contribution_grid::builtins::Strategy;
use contribution_grid::builtins::Theme;
use log::trace;
use serenity::all::ButtonStyle;
use serenity::all::ComponentInteraction;
use serenity::all::CreateActionRow;
use serenity::all::CreateAttachment;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateMediaGallery;
use serenity::all::CreateMediaGalleryItem;
use serenity::all::CreateSeparator;
use serenity::all::CreateTextDisplay;
use serenity::all::CreateUnfurledMediaItem;
use serenity::all::User;

use crate::action_enum;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::settings::SettingsPage;
use crate::bot::commands::settings::run_settings;
use crate::bot::commands::voice::GuildStatType;
use crate::bot::commands::voice::TimeRange;
use crate::bot::commands::voice::VoiceLeaderboardTimeRange;
use crate::bot::commands::voice::VoiceStatsTimeRange;
use crate::bot::commands::voice::views::SettingsVoiceAction;
use crate::bot::commands::voice::views::SettingsVoiceView;
use crate::bot::commands::voice::views::VoiceLeaderboardAction;
use crate::bot::commands::voice::views::VoiceLeaderboardView;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::utils::format_duration;
use crate::bot::views::InteractiveView;
use crate::bot::views::RenderExt;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::controller;
use crate::error::AppError;
use crate::repository::model::GuildDailyStats;
use crate::repository::model::VoiceDailyActivity;
use crate::repository::model::VoiceLeaderboardEntry;
use crate::repository::model::VoiceLeaderboardOptBuilder;
use crate::view_core;

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
        view.render().await?;

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

                    view.render().await?;
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
            view.render().await?;
            return Ok(NavigationResult::Exit);
        }

        // Generate and send initial page
        let page_result = view.generate_current_page().await?;
        view.set_current_page_bytes(page_result.image_bytes.clone());
        view.render().await?;

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
            view.set_current_page_bytes(page_result.image_bytes.clone());
            view.render().await?;
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

// ==================== STATS COMMAND ====================

/// Filename for the voice stats image attachment.
pub const VOICE_STATS_IMAGE_FILENAME: &str = "voice_stats.png";

action_enum! {
    VoiceStatsAction {
        ToggleStatType,
        TimeRange,
    }
}

/// Data for voice stats display.
pub struct VoiceStatsData {
    /// The user this data is for (None for guild stats)
    pub user: Option<User>,
    /// The guild name (for display purposes)
    pub guild_name: String,
    /// Daily activity data for users
    pub user_activity: Vec<VoiceDailyActivity>,
    /// Daily stats for guild (either average time or user count)
    pub guild_stats: Vec<GuildDailyStats>,
    /// Current stat type being displayed (for guild view)
    pub stat_type: GuildStatType,
    /// Time range for the data
    pub time_range: VoiceStatsTimeRange,
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

view_core! {
    timeout = Duration::from_secs(120),
    /// View for displaying voice activity stats.
    pub struct VoiceStatsView<'a, VoiceStatsAction> {
        pub data: VoiceStatsData,
        image_bytes: Option<Vec<u8>>,
    }
}

impl<'a> VoiceStatsView<'a> {
    /// Creates a new stats view.
    pub fn new(ctx: &'a Context<'a>, data: VoiceStatsData) -> Self {
        Self {
            data,
            image_bytes: None,
            core: Self::create_core(ctx),
        }
    }

    /// Sets the generated image bytes.
    pub fn set_image_bytes(&mut self, bytes: Vec<u8>) {
        self.image_bytes = Some(bytes);
    }

    /// Generates the contribution grid image.
    pub fn generate_image(&self) -> anyhow::Result<Vec<u8>> {
        let (since, _until) = self.data.time_range.to_range();
        let today = chrono::Local::now().date_naive();

        // Build data map for contribution grid
        let mut data_map: HashMap<NaiveDate, u32> = HashMap::new();

        if self.data.is_user_stats() {
            // User activity: map day -> total seconds (converted to minutes for display)
            for activity in &self.data.user_activity {
                let minutes = (activity.total_seconds / 60).max(1) as u32;
                data_map.insert(activity.day, minutes);
            }
        } else {
            // Guild stats: map day -> value (minutes for time, count for users)
            for stat in &self.data.guild_stats {
                let value = if self.data.stat_type == GuildStatType::AverageTime {
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
        let (since, until) = self.data.time_range.to_range();
        let time_range_text = format!(
            "-# Time Range: **{}** — <t:{}:f> to <t:{}:R>",
            self.data.time_range.display_name(),
            since.timestamp(),
            until.timestamp(),
        );

        if self.data.is_user_stats() {
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
            // Guild stats - calculate average daily time (same for both modes)
            let avg_time = if self.data.guild_stats.is_empty() {
                0
            } else {
                self.data.guild_stats.iter().map(|s| s.value).sum::<i64>()
                    / self.data.guild_stats.len() as i64
            };
            let _avg_time_str = format_duration(avg_time); // Reserved for future use

            // For guild stats, show different metrics based on stat_type
            let (first_label, first_value, second_label, second_value) = match self.data.stat_type {
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
            };

            format!(
                "### Voice Stats\n{}\n\n**Guild:** {}\n**{}:** {}\n**{}:**{}",
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

impl<'a> ResponseView<'a> for VoiceStatsView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
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

        let time_range_menu = self
            .register(TimeRange)
            .as_select(serenity::all::CreateSelectMenuKind::String {
                options: vec![
                    VoiceStatsTimeRange::ThisWeek.into(),
                    VoiceStatsTimeRange::Past2Weeks.into(),
                    VoiceStatsTimeRange::ThisMonth.into(),
                    VoiceStatsTimeRange::ThisYear.into(),
                ]
                .into(),
            })
            .placeholder("Select time range");

        let mut components = vec![
            CreateComponent::Container(CreateContainer::new(container_components)),
            CreateComponent::ActionRow(CreateActionRow::SelectMenu(time_range_menu)),
        ];

        // Only add button row if there are buttons (guild stats has toggle)
        if !self.data.is_user_stats() {
            let toggle_label = match self.data.stat_type {
                GuildStatType::AverageTime => "Show User Count",
                GuildStatType::ActiveUserCount => "Show Avg Time",
            };
            let button = self
                .register(VoiceStatsAction::ToggleStatType)
                .as_button()
                .label(toggle_label)
                .style(ButtonStyle::Primary);

            components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
                vec![button].into(),
            )));
        }

        components.into()
    }

    fn create_reply<'b>(&mut self) -> poise::CreateReply<'b> {
        let response = self.create_response();
        let mut reply: poise::CreateReply<'b> = response.into();

        if let Some(ref bytes) = self.image_bytes {
            let attachment = CreateAttachment::bytes(bytes.clone(), VOICE_STATS_IMAGE_FILENAME);
            reply = reply.attachment(attachment);
        }

        reply
    }
}

#[async_trait::async_trait]
impl<'a> InteractiveView<'a, VoiceStatsAction> for VoiceStatsView<'a> {
    async fn handle(
        &mut self,
        action: &VoiceStatsAction,
        interaction: &ComponentInteraction,
    ) -> Option<VoiceStatsAction> {
        use VoiceStatsAction::*;

        match action {
            ToggleStatType => {
                // Toggle between stat types
                self.data.stat_type = match self.data.stat_type {
                    GuildStatType::AverageTime => GuildStatType::ActiveUserCount,
                    GuildStatType::ActiveUserCount => GuildStatType::AverageTime,
                };
                Some(action.clone())
            }
            TimeRange => {
                if let serenity::all::ComponentInteractionDataKind::StringSelect { values } =
                    &interaction.data.kind
                    && let Some(time_range) = values
                        .first()
                        .and_then(|v| VoiceStatsTimeRange::from_display_name(v))
                    && self.data.time_range != time_range
                {
                    self.data.time_range = time_range;
                    return Some(action.clone());
                }
                None
            }
        }
    }
}

/// Controller for voice stats display and interaction.
pub struct VoiceStatsController<'a> {
    #[allow(dead_code)]
    ctx: &'a Context<'a>,
    pub time_range: VoiceStatsTimeRange,
    pub target_user: Option<User>,
    pub stat_type: GuildStatType,
}

impl<'a> VoiceStatsController<'a> {
    /// Creates a new stats controller.
    pub fn new(
        ctx: &'a Context<'a>,
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
        let (since, until) = self.time_range.to_range();

        // Get guild info
        let guild_name = if let Some(guild) = ctx.guild() {
            guild.name.to_string()
        } else {
            "Direct Messages".to_string()
        };

        if let Some(ref target_user) = self.target_user {
            // Fetch user-specific stats
            let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

            let user_activity = ctx
                .data()
                .service
                .voice_tracking
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
            })
        } else {
            // Fetch guild-wide stats
            let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

            let guild_stats = ctx
                .data()
                .service
                .voice_tracking
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
            })
        }
    }
}

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for VoiceStatsController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let controller_start = Instant::now();

        let ctx = *coordinator.context();
        ctx.defer().await?;

        // Fetch initial data
        let data = self.fetch_data(&ctx).await?;
        let mut view = VoiceStatsView::new(&ctx, data);

        // Generate and send the image
        if !view.data.user_activity.is_empty() || !view.data.guild_stats.is_empty() {
            match view.generate_image() {
                Ok(bytes) => {
                    view.set_image_bytes(bytes);
                }
                Err(e) => {
                    log::error!("Failed to generate stats image: {}", e);
                }
            }
        }

        view.render().await?;

        trace!(
            "stats_controller_initial_response {} ms",
            controller_start.elapsed().as_millis()
        );

        while let Some((action, _interaction)) = view.listen_once().await? {
            match action {
                VoiceStatsAction::ToggleStatType | VoiceStatsAction::TimeRange => {
                    // Update controller state from view
                    self.time_range = view.data.time_range;
                    self.stat_type = view.data.stat_type;

                    // Re-fetch data with new parameters
                    let new_data = self.fetch_data(&ctx).await?;
                    view.data = new_data;

                    // Regenerate image
                    if !view.data.user_activity.is_empty() || !view.data.guild_stats.is_empty() {
                        match view.generate_image() {
                            Ok(bytes) => {
                                view.set_image_bytes(bytes);
                            }
                            Err(e) => {
                                log::error!("Failed to regenerate stats image: {}", e);
                            }
                        }
                    }

                    view.render().await?;
                }
            }
        }

        trace!(
            "stats_controller_total {} ms",
            controller_start.elapsed().as_millis()
        );
        Ok(NavigationResult::Exit)
    }
}

/// Entry point for the stats command.
pub async fn stats(
    ctx: Context<'_>,
    time_range: Option<VoiceStatsTimeRange>,
    user: Option<User>,
    statistic: Option<GuildStatType>,
) -> Result<(), Error> {
    let time_range = time_range.unwrap_or(VoiceStatsTimeRange::ThisMonth);
    let stat_type = statistic.unwrap_or_default();

    // Determine target user and context
    let target_user = if let Some(_guild_id) = ctx.guild_id() {
        // In guild context
        if let Some(ref target) = user {
            // Allow viewing own stats, otherwise check membership via Discord API
            if target.id != ctx.author().id {
                // Try to get member from cache first
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
            // No user specified - show guild stats
            None
        }
    } else {
        // In DM context - can only view own stats
        if let Some(ref target) = user {
            if target.id != ctx.author().id {
                return Err(BotError::UserNotInGuild(
                    "In direct messages, you can only view your own voice stats.".to_string(),
                )
                .into());
            }
            Some(target.clone())
        } else {
            // No user specified in DM - show own stats
            Some(ctx.author().clone())
        }
    };

    let mut coordinator = Coordinator::new(ctx);
    let mut controller = VoiceStatsController::new(&ctx, time_range, target_user, stat_type);
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
