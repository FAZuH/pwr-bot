//! Pure update logic for the voice stats view.
//!
//! All state mutations — time range changes, stat type switches, and user/guild
//! mode toggling — live here so they can be unit-tested without touching
//! Discord or the database.

use crate::bot::command::voice::GuildStatType;
use crate::bot::command::voice::VoiceStatsTimeRange;
use crate::update::Update;

/// Messages that can mutate the voice-stats model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceStatsMsg {
    /// Switch to a different time range.
    ChangeTimeRange(VoiceStatsTimeRange),
    /// Switch to a different guild stat type.
    ChangeStatType(GuildStatType),
    /// Toggle between user stats and guild stats.
    ToggleDataMode,
    /// Set (or clear) the target user.
    SetUser(Option<u64>),
}

/// Commands returned by the update, instructing the caller what to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceStatsCmd {
    /// Nothing else to do.
    None,
    /// The caller should re-fetch stats data from the database.
    RefetchData,
}

/// The voice-stats model — everything that determines what data is fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceStatsModel {
    pub time_range: VoiceStatsTimeRange,
    pub stat_type: GuildStatType,
    /// `Some(user_id)` = user stats, `None` = guild stats.
    pub user_id: Option<u64>,
    /// The user to fall back to when toggling from guild → user mode.
    pub fallback_user_id: u64,
}

impl VoiceStatsModel {
    /// Creates a new model in guild-stats mode.
    pub fn new(fallback_user_id: u64) -> Self {
        Self {
            time_range: VoiceStatsTimeRange::default(),
            stat_type: GuildStatType::default(),
            user_id: None,
            fallback_user_id,
        }
    }

    /// Returns `true` when the model is configured for user stats.
    pub fn is_user_stats(&self) -> bool {
        self.user_id.is_some()
    }
}

/// The update implementation for voice stats.
#[derive(Debug, Clone, Copy, Default)]
pub struct VoiceStatsUpdate;

impl VoiceStatsUpdate {
    pub fn new() -> Self {
        Self
    }
}

impl Update for VoiceStatsUpdate {
    type Model = VoiceStatsModel;
    type Msg = VoiceStatsMsg;
    type Cmd = VoiceStatsCmd;

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Self::Cmd {
        use VoiceStatsMsg::*;

        match msg {
            ChangeTimeRange(range) => {
                if model.time_range != range {
                    model.time_range = range;
                    VoiceStatsCmd::RefetchData
                } else {
                    VoiceStatsCmd::None
                }
            }
            ChangeStatType(stat) => {
                if model.stat_type != stat {
                    model.stat_type = stat;
                    VoiceStatsCmd::RefetchData
                } else {
                    VoiceStatsCmd::None
                }
            }
            ToggleDataMode => {
                if model.is_user_stats() {
                    model.user_id = None;
                } else {
                    model.user_id = Some(model.fallback_user_id);
                }
                VoiceStatsCmd::RefetchData
            }
            SetUser(user_id) => {
                if let Some(id) = user_id {
                    model.fallback_user_id = id;
                }
                if model.user_id != user_id {
                    model.user_id = user_id;
                    VoiceStatsCmd::RefetchData
                } else {
                    VoiceStatsCmd::None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ChangeTimeRange ─────────────────────────────────────────────────────

    #[test]
    fn test_change_time_range_updates_and_refetches() {
        let mut model = VoiceStatsModel::new(100);
        assert_eq!(model.time_range, VoiceStatsTimeRange::Yearly);

        let cmd = VoiceStatsUpdate::update(
            VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Monthly),
            &mut model,
        );

        assert_eq!(cmd, VoiceStatsCmd::RefetchData);
        assert_eq!(model.time_range, VoiceStatsTimeRange::Monthly);
    }

    #[test]
    fn test_change_time_range_same_returns_none() {
        let mut model = VoiceStatsModel::new(100);
        model.time_range = VoiceStatsTimeRange::Monthly;

        let cmd = VoiceStatsUpdate::update(
            VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Monthly),
            &mut model,
        );

        assert_eq!(cmd, VoiceStatsCmd::None);
    }

    // ── ChangeStatType ──────────────────────────────────────────────────────

    #[test]
    fn test_change_stat_type_updates_and_refetches() {
        let mut model = VoiceStatsModel::new(100);
        assert_eq!(model.stat_type, GuildStatType::AverageTime);

        let cmd = VoiceStatsUpdate::update(
            VoiceStatsMsg::ChangeStatType(GuildStatType::TotalTime),
            &mut model,
        );

        assert_eq!(cmd, VoiceStatsCmd::RefetchData);
        assert_eq!(model.stat_type, GuildStatType::TotalTime);
    }

    #[test]
    fn test_change_stat_type_same_returns_none() {
        let mut model = VoiceStatsModel::new(100);
        model.stat_type = GuildStatType::ActiveUserCount;

        let cmd = VoiceStatsUpdate::update(
            VoiceStatsMsg::ChangeStatType(GuildStatType::ActiveUserCount),
            &mut model,
        );

        assert_eq!(cmd, VoiceStatsCmd::None);
    }

    // ── ToggleDataMode ──────────────────────────────────────────────────────

    #[test]
    fn test_toggle_from_guild_to_user() {
        let mut model = VoiceStatsModel::new(100);
        assert!(!model.is_user_stats());

        let cmd = VoiceStatsUpdate::update(VoiceStatsMsg::ToggleDataMode, &mut model);

        assert_eq!(cmd, VoiceStatsCmd::RefetchData);
        assert!(model.is_user_stats());
        assert_eq!(model.user_id, Some(100));
    }

    #[test]
    fn test_toggle_from_user_to_guild() {
        let mut model = VoiceStatsModel::new(100);
        model.user_id = Some(100);

        let cmd = VoiceStatsUpdate::update(VoiceStatsMsg::ToggleDataMode, &mut model);

        assert_eq!(cmd, VoiceStatsCmd::RefetchData);
        assert!(!model.is_user_stats());
        assert_eq!(model.user_id, None);
    }

    #[test]
    fn test_toggle_uses_current_fallback() {
        let mut model = VoiceStatsModel::new(100);
        model.fallback_user_id = 200;

        VoiceStatsUpdate::update(VoiceStatsMsg::ToggleDataMode, &mut model);
        assert_eq!(model.user_id, Some(200));
    }

    // ── SetUser ─────────────────────────────────────────────────────────────

    #[test]
    fn test_set_user_changes_target_and_refetches() {
        let mut model = VoiceStatsModel::new(100);

        let cmd = VoiceStatsUpdate::update(VoiceStatsMsg::SetUser(Some(200)), &mut model);

        assert_eq!(cmd, VoiceStatsCmd::RefetchData);
        assert_eq!(model.user_id, Some(200));
        assert_eq!(model.fallback_user_id, 200);
    }

    #[test]
    fn test_set_user_same_returns_none() {
        let mut model = VoiceStatsModel::new(100);
        model.user_id = Some(200);

        let cmd = VoiceStatsUpdate::update(VoiceStatsMsg::SetUser(Some(200)), &mut model);

        assert_eq!(cmd, VoiceStatsCmd::None);
        assert_eq!(model.fallback_user_id, 200); // still updated
    }

    #[test]
    fn test_set_user_clear() {
        let mut model = VoiceStatsModel::new(100);
        model.user_id = Some(200);

        let cmd = VoiceStatsUpdate::update(VoiceStatsMsg::SetUser(None), &mut model);

        assert_eq!(cmd, VoiceStatsCmd::RefetchData);
        assert_eq!(model.user_id, None);
    }

    #[test]
    fn test_set_user_none_to_none() {
        let mut model = VoiceStatsModel::new(100);
        model.user_id = None;

        let cmd = VoiceStatsUpdate::update(VoiceStatsMsg::SetUser(None), &mut model);

        assert_eq!(cmd, VoiceStatsCmd::None);
    }

    #[test]
    fn test_set_user_updates_fallback_only_when_some() {
        let mut model = VoiceStatsModel::new(100);

        VoiceStatsUpdate::update(VoiceStatsMsg::SetUser(None), &mut model);
        assert_eq!(model.fallback_user_id, 100); // unchanged
    }

    // ── Model helpers ───────────────────────────────────────────────────────

    #[test]
    fn test_is_user_stats() {
        let mut model = VoiceStatsModel::new(100);
        assert!(!model.is_user_stats());

        model.user_id = Some(200);
        assert!(model.is_user_stats());

        model.user_id = None;
        assert!(!model.is_user_stats());
    }

    #[test]
    fn test_new_defaults_to_guild_mode() {
        let model = VoiceStatsModel::new(42);
        assert_eq!(model.user_id, None);
        assert_eq!(model.fallback_user_id, 42);
        assert_eq!(model.time_range, VoiceStatsTimeRange::Yearly);
        assert_eq!(model.stat_type, GuildStatType::AverageTime);
    }
}
