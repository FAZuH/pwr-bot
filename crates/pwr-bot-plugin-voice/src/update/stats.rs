/// Stat type for guild-wide or user-specific voice stats (local domain type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GuildStatType {
    #[default]
    AverageTime,
    ActiveUserCount,
    TotalTime,
}

/// Time range for voice stats (local domain type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceStatsTimeRange {
    #[default]
    Yearly,
    Monthly,
    Weekly,
    Hourly,
}

/// Messages that can mutate the voice stats model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceStatsMsg {
    ChangeTimeRange(VoiceStatsTimeRange),
    ChangeStatType(GuildStatType),
    ToggleDataMode,
    SetUser(Option<u64>),
}

/// Commands returned by the update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceStatsCmd {
    None,
    RefetchData,
}

/// The voice stats model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceStatsModel {
    pub time_range: VoiceStatsTimeRange,
    pub stat_type: GuildStatType,
    pub user_id: Option<u64>,
    pub fallback_user_id: Option<u64>,
}

impl VoiceStatsModel {
    pub fn new(user_id: Option<u64>) -> Self {
        Self {
            time_range: VoiceStatsTimeRange::default(),
            stat_type: GuildStatType::default(),
            user_id,
            fallback_user_id: user_id,
        }
    }

    pub fn is_user_stats(&self) -> bool {
        self.user_id.is_some()
    }
}

/// Pure update function for voice stats.
pub fn voice_stats_update(msg: VoiceStatsMsg, model: &mut VoiceStatsModel) -> VoiceStatsCmd {
    use VoiceStatsCmd::*;
    use VoiceStatsMsg::*;

    match msg {
        ChangeTimeRange(range) => {
            if model.time_range != range {
                model.time_range = range;
                RefetchData
            } else {
                None
            }
        }
        ChangeStatType(stat_type) => {
            if model.stat_type != stat_type {
                model.stat_type = stat_type;
                RefetchData
            } else {
                None
            }
        }
        ToggleDataMode => {
            model.user_id = if model.user_id.is_some() {
                Option::<u64>::None
            } else {
                model.fallback_user_id
            };
            RefetchData
        }
        SetUser(user_id) => {
            model.user_id = user_id;
            if user_id.is_some() {
                model.fallback_user_id = user_id;
                RefetchData
            } else {
                VoiceStatsCmd::None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults_to_guild_mode() {
        let model = VoiceStatsModel::new(None);
        assert!(!model.is_user_stats());
        assert_eq!(model.time_range, VoiceStatsTimeRange::Yearly);
        assert_eq!(model.stat_type, GuildStatType::AverageTime);
    }

    #[test]
    fn change_time_range_updates_and_refetches() {
        let mut model = VoiceStatsModel::new(None);
        let cmd = voice_stats_update(
            VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Monthly),
            &mut model,
        );
        assert_eq!(cmd, VoiceStatsCmd::RefetchData);
        assert_eq!(model.time_range, VoiceStatsTimeRange::Monthly);
    }

    #[test]
    fn change_time_range_same_returns_none() {
        let mut model = VoiceStatsModel::new(None);
        let cmd = voice_stats_update(
            VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Yearly),
            &mut model,
        );
        assert_eq!(cmd, VoiceStatsCmd::None);
    }

    #[test]
    fn change_stat_type_updates_and_refetches() {
        let mut model = VoiceStatsModel::new(None);
        let cmd = voice_stats_update(
            VoiceStatsMsg::ChangeStatType(GuildStatType::TotalTime),
            &mut model,
        );
        assert_eq!(cmd, VoiceStatsCmd::RefetchData);
        assert_eq!(model.stat_type, GuildStatType::TotalTime);
    }

    #[test]
    fn change_stat_type_same_returns_none() {
        let mut model = VoiceStatsModel::new(None);
        let cmd = voice_stats_update(
            VoiceStatsMsg::ChangeStatType(GuildStatType::AverageTime),
            &mut model,
        );
        assert_eq!(cmd, VoiceStatsCmd::None);
    }

    #[test]
    fn toggle_from_guild_to_user() {
        let mut model = VoiceStatsModel::new(Some(42));
        model.user_id = None;
        let cmd = voice_stats_update(VoiceStatsMsg::ToggleDataMode, &mut model);
        assert_eq!(cmd, VoiceStatsCmd::RefetchData);
        assert_eq!(model.user_id, Some(42));
    }

    #[test]
    fn toggle_from_user_to_guild() {
        let mut model = VoiceStatsModel::new(Some(42));
        let cmd = voice_stats_update(VoiceStatsMsg::ToggleDataMode, &mut model);
        assert_eq!(cmd, VoiceStatsCmd::RefetchData);
        assert!(model.user_id.is_none());
    }

    #[test]
    fn toggle_uses_current_fallback() {
        let mut model = VoiceStatsModel::new(Some(42));
        // Set user to guild mode, then toggle back
        voice_stats_update(VoiceStatsMsg::ToggleDataMode, &mut model);
        assert!(model.user_id.is_none());

        voice_stats_update(VoiceStatsMsg::ToggleDataMode, &mut model);
        assert_eq!(model.user_id, Some(42));
    }

    #[test]
    fn set_user_changes_target_and_refetches() {
        let mut model = VoiceStatsModel::new(None);
        let cmd = voice_stats_update(VoiceStatsMsg::SetUser(Some(99)), &mut model);
        assert_eq!(cmd, VoiceStatsCmd::RefetchData);
        assert_eq!(model.user_id, Some(99));
        assert_eq!(model.fallback_user_id, Some(99));
    }

    #[test]
    fn set_user_clear() {
        let mut model = VoiceStatsModel::new(Some(42));
        let cmd = voice_stats_update(VoiceStatsMsg::SetUser(None), &mut model);
        assert_eq!(cmd, VoiceStatsCmd::None);
        assert!(model.user_id.is_none());
    }

    #[test]
    fn set_user_none_to_none() {
        let mut model = VoiceStatsModel::new(None);
        let cmd = voice_stats_update(VoiceStatsMsg::SetUser(None), &mut model);
        assert_eq!(cmd, VoiceStatsCmd::None);
        assert!(model.user_id.is_none());
    }

    #[test]
    fn set_user_same_returns_refetch() {
        let mut model = VoiceStatsModel::new(Some(42));
        let cmd = voice_stats_update(VoiceStatsMsg::SetUser(Some(42)), &mut model);
        assert_eq!(cmd, VoiceStatsCmd::RefetchData);
        assert_eq!(model.user_id, Some(42));
    }

    #[test]
    fn set_user_updates_fallback_only_when_some() {
        let mut model = VoiceStatsModel::new(Some(100));
        voice_stats_update(VoiceStatsMsg::SetUser(Some(200)), &mut model);
        assert_eq!(model.fallback_user_id, Some(200));

        voice_stats_update(VoiceStatsMsg::SetUser(None), &mut model);
        assert_eq!(model.fallback_user_id, Some(200));
    }
}
