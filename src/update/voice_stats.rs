//! Pure update logic for the voice stats view.
//!
//! All state mutations — time range changes, stat type switches, user/guild
//! mode toggling, and the display data themselves — live here so they can be
//! unit-tested without touching Discord or the database. The model is the
//! single source of truth for the view, absorbing the fetched stats data and
//! the rendered image bytes (raw `Vec<u8>`, no serenity types cross in).

use crate::bot::command::voice::GuildStatType;
use crate::bot::command::voice::VoiceStatsTimeRange;
use crate::entity::GuildDailyStats;
use crate::entity::VoiceDailyActivity;
use crate::entity::VoiceSessionsEntity;
use crate::update::lifecycle::Lifecycle;

/// The fetched display data for the voice stats view.
///
/// This is the domain snapshot rendered by the view. It carries no serenity
/// types: the target user is represented by its id and display-name string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceStatsData {
    /// The server name (for display purposes).
    pub guild_name: String,
    /// Daily activity data for users.
    pub user_activity: Vec<VoiceDailyActivity>,
    /// Daily stats for the guild (either average time or user count).
    pub guild_stats: Vec<GuildDailyStats>,
    /// Raw sessions for line-chart generation.
    pub raw_sessions: Vec<VoiceSessionsEntity>,
    /// The resolved display name of the target user (user mode only).
    pub target_user_name: Option<String>,
}

/// Messages that can mutate the voice-stats model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceStatsMsg {
    /// The shared boot/timeout lifecycle moments.
    Lifecycle(Lifecycle),
    /// Switch to a different time range.
    ChangeTimeRange(VoiceStatsTimeRange),
    /// Switch to a different guild stat type.
    ChangeStatType(GuildStatType),
    /// Toggle between user stats and guild stats.
    ToggleDataMode,
    /// Set (or clear) the target user.
    SetUser(Option<u64>),
    /// The adapter loaded a fresh stats snapshot.
    StatsLoaded(VoiceStatsData),
    /// The adapter finished (re)rendering the image.
    ImageRendered(Option<Vec<u8>>),
}

impl From<Lifecycle> for VoiceStatsMsg {
    fn from(lifecycle: Lifecycle) -> Self {
        Self::Lifecycle(lifecycle)
    }
}

/// Effects the voice-stats view can request.
///
/// Data-only. In-session stat fetching and image rendering are both driven
/// through [`VoiceStatsEffect`], executed by the shell adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceStatsEffect {
    /// Refetch the stats data for the given parameters.
    QueryStats {
        time_range: VoiceStatsTimeRange,
        stat_type: GuildStatType,
        user_id: Option<u64>,
    },
    /// (Re)render the chart image from a snapshot of the current data.
    RenderImage {
        time_range: VoiceStatsTimeRange,
        stat_type: GuildStatType,
        is_user: bool,
        raw_sessions: Vec<VoiceSessionsEntity>,
        user_activity: Vec<VoiceDailyActivity>,
        guild_stats: Vec<GuildDailyStats>,
    },
}

/// The voice-stats model — everything that determines what is displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceStatsModel {
    pub(crate) time_range: VoiceStatsTimeRange,
    pub(crate) stat_type: GuildStatType,
    /// `Some(user_id)` = user stats, `None` = guild stats.
    pub(crate) user_id: Option<u64>,
    /// The user to fall back to when toggling from guild → user mode.
    pub(crate) fallback_user_id: u64,
    pub(crate) data: VoiceStatsData,
    pub(crate) image_bytes: Option<Vec<u8>>,
}

impl VoiceStatsModel {
    /// Creates a voice-stats model from the boot-time data and image.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        time_range: VoiceStatsTimeRange,
        stat_type: GuildStatType,
        user_id: Option<u64>,
        fallback_user_id: u64,
        data: VoiceStatsData,
        image_bytes: Option<Vec<u8>>,
    ) -> Self {
        Self {
            time_range,
            stat_type,
            user_id,
            fallback_user_id,
            data,
            image_bytes,
        }
    }

    /// Returns `true` when the model is configured for user stats.
    pub fn is_user_stats(&self) -> bool {
        self.user_id.is_some()
    }

    /// The active time range.
    pub fn time_range(&self) -> VoiceStatsTimeRange {
        self.time_range
    }

    /// The active guild stat type.
    pub fn stat_type(&self) -> GuildStatType {
        self.stat_type
    }

    /// The target user id, if in user mode.
    pub fn user_id(&self) -> Option<u64> {
        self.user_id
    }

    /// The displayed image bytes, if the image was rendered.
    pub fn image_bytes(&self) -> Option<&[u8]> {
        self.image_bytes.as_deref()
    }

    /// The server name.
    pub fn guild_name(&self) -> &str {
        &self.data.guild_name
    }

    /// The display name for the stats subject (user in user mode, guild otherwise).
    pub fn display_name(&self) -> &str {
        self.data
            .target_user_name
            .as_deref()
            .unwrap_or(&self.data.guild_name)
    }

    /// Calculates total time from user activity data.
    pub fn total_time(&self) -> i64 {
        self.data
            .user_activity
            .iter()
            .map(|a| a.total_seconds)
            .sum()
    }

    /// Calculates average daily time.
    pub fn average_daily_time(&self) -> i64 {
        if self.data.user_activity.is_empty() {
            return 0;
        }
        self.total_time() / self.data.user_activity.len() as i64
    }

    /// Finds the most active day.
    pub fn most_active_day(&self) -> Option<(chrono::NaiveDate, i64)> {
        self.data
            .user_activity
            .iter()
            .max_by_key(|a| a.total_seconds)
            .map(|a| (a.day, a.total_seconds))
    }

    /// Calculates the current streak (consecutive days with activity up to today).
    pub fn current_streak(&self) -> u32 {
        if self.data.user_activity.is_empty() {
            return 0;
        }

        let today = chrono::Local::now().date_naive();
        let mut streak = 0;

        // Sort by date descending
        let mut sorted: Vec<_> = self.data.user_activity.iter().map(|a| a.day).collect();
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

    /// The daily guild stats (guild mode only).
    pub fn guild_stats(&self) -> &[GuildDailyStats] {
        &self.data.guild_stats
    }

    /// The daily user activity (user mode only).
    pub fn user_activity(&self) -> &[VoiceDailyActivity] {
        &self.data.user_activity
    }

    /// Gets the maximum value for guild stats (for scaling).
    pub fn max_guild_stat_value(&self) -> i64 {
        self.data
            .guild_stats
            .iter()
            .map(|s| s.value)
            .max()
            .unwrap_or(0)
    }

    /// Gets the total for guild user-count stats.
    pub fn total_active_users(&self) -> i64 {
        self.data.guild_stats.iter().map(|s| s.value).sum()
    }
}

/// The pure update function — the only writer of the voice-stats model.
///
/// `Start` is a no-op (initial data and image arrive via the feature config).
/// Each state-change message returns a [`VoiceStatsEffect::QueryStats`] so the
/// adapter refetches; the returned [`VoiceStatsMsg::StatsLoaded`] replaces the
/// display data and requests a [`VoiceStatsEffect::RenderImage`].
pub fn update(msg: VoiceStatsMsg, model: &mut VoiceStatsModel) -> Vec<VoiceStatsEffect> {
    use VoiceStatsMsg::*;

    match msg {
        VoiceStatsMsg::Lifecycle(lifecycle) => lifecycle.handle(Vec::new),
        ChangeTimeRange(range) => {
            if model.time_range != range {
                model.time_range = range;
                vec![query(model)]
            } else {
                Vec::new()
            }
        }
        ChangeStatType(stat) => {
            if model.stat_type != stat {
                model.stat_type = stat;
                vec![query(model)]
            } else {
                Vec::new()
            }
        }
        ToggleDataMode => {
            if model.is_user_stats() {
                model.user_id = None;
            } else {
                model.user_id = Some(model.fallback_user_id);
            }
            vec![query(model)]
        }
        SetUser(user_id) => {
            if let Some(id) = user_id {
                model.fallback_user_id = id;
            }
            if model.user_id != user_id {
                model.user_id = user_id;
                vec![query(model)]
            } else {
                Vec::new()
            }
        }
        StatsLoaded(data) => {
            model.data = data;
            vec![render_image(model)]
        }
        ImageRendered(bytes) => {
            model.image_bytes = bytes;
            Vec::new()
        }
    }
}

/// Builds a [`VoiceStatsEffect::QueryStats`] from the current model.
fn query(model: &VoiceStatsModel) -> VoiceStatsEffect {
    VoiceStatsEffect::QueryStats {
        time_range: model.time_range,
        stat_type: model.stat_type,
        user_id: model.user_id,
    }
}

/// Builds a [`VoiceStatsEffect::RenderImage`] from the current display data.
fn render_image(model: &VoiceStatsModel) -> VoiceStatsEffect {
    VoiceStatsEffect::RenderImage {
        time_range: model.time_range,
        stat_type: model.stat_type,
        is_user: model.is_user_stats(),
        raw_sessions: model.data.raw_sessions.clone(),
        user_activity: model.data.user_activity.clone(),
        guild_stats: model.data.guild_stats.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(user_id: u64) -> VoiceSessionsEntity {
        VoiceSessionsEntity {
            id: user_id as i32,
            user_id,
            guild_id: 1,
            channel_id: 1,
            join_time: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            leave_time: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            is_active: false,
        }
    }

    fn data() -> VoiceStatsData {
        VoiceStatsData {
            guild_name: "Test Server".to_string(),
            user_activity: vec![],
            guild_stats: vec![],
            raw_sessions: vec![],
            target_user_name: None,
        }
    }

    fn model(fallback: u64) -> VoiceStatsModel {
        VoiceStatsModel::new(
            VoiceStatsTimeRange::default(),
            GuildStatType::default(),
            None,
            fallback,
            data(),
            None,
        )
    }

    // ── ChangeTimeRange ─────────────────────────────────────────────────────

    #[test]
    fn change_time_range_updates_and_queries() {
        let mut model = model(100);
        assert_eq!(model.time_range(), VoiceStatsTimeRange::Yearly);

        let effects = update(
            VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Monthly),
            &mut model,
        );

        assert_eq!(
            effects,
            vec![VoiceStatsEffect::QueryStats {
                time_range: VoiceStatsTimeRange::Monthly,
                stat_type: GuildStatType::default(),
                user_id: None,
            }]
        );
        assert_eq!(model.time_range(), VoiceStatsTimeRange::Monthly);
    }

    #[test]
    fn change_time_range_same_returns_empty() {
        let mut model = model(100);
        model.time_range = VoiceStatsTimeRange::Monthly;

        let effects = update(
            VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Monthly),
            &mut model,
        );

        assert!(effects.is_empty());
    }

    // ── ChangeStatType ──────────────────────────────────────────────────────

    #[test]
    fn change_stat_type_updates_and_queries() {
        let mut model = model(100);
        assert_eq!(model.stat_type(), GuildStatType::AverageTime);

        let effects = update(
            VoiceStatsMsg::ChangeStatType(GuildStatType::TotalTime),
            &mut model,
        );

        assert_eq!(
            effects,
            vec![VoiceStatsEffect::QueryStats {
                time_range: VoiceStatsTimeRange::default(),
                stat_type: GuildStatType::TotalTime,
                user_id: None,
            }]
        );
        assert_eq!(model.stat_type(), GuildStatType::TotalTime);
    }

    #[test]
    fn change_stat_type_same_returns_empty() {
        let mut model = model(100);
        model.stat_type = GuildStatType::ActiveUserCount;

        let effects = update(
            VoiceStatsMsg::ChangeStatType(GuildStatType::ActiveUserCount),
            &mut model,
        );

        assert!(effects.is_empty());
    }

    // ── ToggleDataMode ──────────────────────────────────────────────────────

    #[test]
    fn toggle_from_guild_to_user() {
        let mut model = model(100);
        assert!(!model.is_user_stats());

        let effects = update(VoiceStatsMsg::ToggleDataMode, &mut model);

        assert_eq!(effects.len(), 1);
        assert!(model.is_user_stats());
        assert_eq!(model.user_id(), Some(100));
    }

    #[test]
    fn toggle_from_user_to_guild() {
        let mut model = model(100);
        model.user_id = Some(100);

        let effects = update(VoiceStatsMsg::ToggleDataMode, &mut model);

        assert_eq!(effects.len(), 1);
        assert!(!model.is_user_stats());
        assert_eq!(model.user_id(), None);
    }

    #[test]
    fn toggle_uses_current_fallback() {
        let mut model = model(100);
        model.fallback_user_id = 200;

        update(VoiceStatsMsg::ToggleDataMode, &mut model);
        assert_eq!(model.user_id(), Some(200));
    }

    // ── SetUser ─────────────────────────────────────────────────────────────

    #[test]
    fn set_user_changes_target_and_queries() {
        let mut model = model(100);

        let effects = update(VoiceStatsMsg::SetUser(Some(200)), &mut model);

        assert_eq!(effects.len(), 1);
        assert_eq!(model.user_id(), Some(200));
        assert_eq!(model.fallback_user_id, 200);
    }

    #[test]
    fn set_user_same_returns_empty() {
        let mut model = model(100);
        model.user_id = Some(200);

        let effects = update(VoiceStatsMsg::SetUser(Some(200)), &mut model);

        assert!(effects.is_empty());
        assert_eq!(model.fallback_user_id, 200);
    }

    #[test]
    fn set_user_clear() {
        let mut model = model(100);
        model.user_id = Some(200);

        let effects = update(VoiceStatsMsg::SetUser(None), &mut model);

        assert_eq!(effects.len(), 1);
        assert_eq!(model.user_id(), None);
    }

    #[test]
    fn set_user_none_to_none() {
        let mut model = model(100);

        let effects = update(VoiceStatsMsg::SetUser(None), &mut model);

        assert!(effects.is_empty());
    }

    // ── StatsLoaded / ImageRendered ─────────────────────────────────────────

    #[test]
    fn stats_loaded_replaces_data_and_requests_image() {
        let mut model = model(100);
        let new_data = VoiceStatsData {
            guild_name: "New Server".to_string(),
            user_activity: vec![VoiceDailyActivity {
                day: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                total_seconds: 3600,
            }],
            guild_stats: vec![],
            raw_sessions: vec![entry(1)],
            target_user_name: Some("tester".to_string()),
        };

        let effects = update(VoiceStatsMsg::StatsLoaded(new_data.clone()), &mut model);

        assert_eq!(effects.len(), 1);
        match &effects[0] {
            VoiceStatsEffect::RenderImage {
                is_user,
                raw_sessions,
                user_activity,
                ..
            } => {
                assert!(!is_user);
                assert_eq!(*raw_sessions, vec![entry(1)]);
                assert_eq!(user_activity.len(), 1);
            }
            other => panic!("expected RenderImage, got {other:?}"),
        }
        assert_eq!(model.guild_name(), "New Server");
        assert_eq!(model.display_name(), "tester");
        assert_eq!(model.total_time(), 3600);
    }

    #[test]
    fn stats_loaded_guild_mode_display_name_is_guild() {
        let mut model = model(100);
        let guild_data = data();
        update(VoiceStatsMsg::StatsLoaded(guild_data), &mut model);
        assert_eq!(model.display_name(), "Test Server");
    }

    #[test]
    fn image_rendered_sets_bytes() {
        let mut model = model(100);
        let effects = update(
            VoiceStatsMsg::ImageRendered(Some(vec![1, 2, 3])),
            &mut model,
        );
        assert!(effects.is_empty());
        assert_eq!(model.image_bytes(), Some(&[1, 2, 3][..]));
    }

    #[test]
    fn start_is_noop() {
        let mut model = model(100);
        let effects = update(VoiceStatsMsg::Lifecycle(Lifecycle::Start), &mut model);
        assert!(effects.is_empty());
    }

    #[test]
    fn expired_is_noop() {
        let mut model = model(100);
        let effects = update(VoiceStatsMsg::Lifecycle(Lifecycle::Expired), &mut model);
        assert!(effects.is_empty());
    }

    // ── Model helpers ───────────────────────────────────────────────────────

    #[test]
    fn is_user_stats() {
        let mut model = model(100);
        assert!(!model.is_user_stats());

        model.user_id = Some(200);
        assert!(model.is_user_stats());
    }

    #[test]
    fn current_streak_counts_consecutive_days() {
        let mut model = model(100);
        let today = chrono::Local::now().date_naive();
        model.data.user_activity = vec![
            VoiceDailyActivity {
                day: today,
                total_seconds: 100,
            },
            VoiceDailyActivity {
                day: today.pred_opt().unwrap(),
                total_seconds: 100,
            },
        ];
        assert_eq!(model.current_streak(), 2);
    }

    #[test]
    fn average_daily_time_empty_is_zero() {
        let model = model(100);
        assert_eq!(model.average_daily_time(), 0);
    }

    #[test]
    fn total_active_users_sums() {
        let mut model = model(100);
        model.data.guild_stats = vec![
            GuildDailyStats {
                day: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                value: 3,
            },
            GuildDailyStats {
                day: chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
                value: 5,
            },
        ];
        assert_eq!(model.total_active_users(), 8);
    }
}
