use crate::update::PaginationAction;
use crate::update::PaginationModel;

/// A leaderboard entry (local domain type, no Diesel dependency).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoiceLeaderboardEntry {
    pub user_id: u64,
    pub total_duration: i64,
}

/// Time range for voice leaderboard queries (local type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceLeaderboardTimeRange {
    #[default]
    ThisMonth,
    Today,
    Past24Hours,
    Past72Hours,
    Past7Days,
    Past14Days,
    ThisYear,
    AllTime,
}

/// Messages that can mutate the leaderboard model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceLeaderboardMsg {
    SetEntries(Vec<VoiceLeaderboardEntry>),
    ChangeTimeRange(VoiceLeaderboardTimeRange),
    ToggleMode,
    SetTargetUser(Option<u64>),
    Pagination(PaginationAction),
}

/// Commands returned by the update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceLeaderboardCmd {
    None,
    RefetchData,
}

/// The leaderboard model — everything needed to render a page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VoiceLeaderboardModel {
    pub entries: Vec<VoiceLeaderboardEntry>,
    pub user_rank: Option<u32>,
    pub user_duration: Option<i64>,
    pub time_range: VoiceLeaderboardTimeRange,
    pub partner_mode: bool,
    pub target_user: Option<u64>,
    pub pagination: PaginationModel,
}

impl VoiceLeaderboardModel {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Pure update function for the voice leaderboard.
pub fn voice_leaderboard_update(
    msg: VoiceLeaderboardMsg,
    model: &mut VoiceLeaderboardModel,
) -> VoiceLeaderboardCmd {
    use VoiceLeaderboardCmd::*;
    use VoiceLeaderboardMsg::*;

    match msg {
        SetEntries(entries) => {
            model.entries = entries;
            let user_entries = model.entries.clone();
            let total = user_entries.len() as u32;
            model.pagination = PaginationModel::new(total, 1);
            None
        }
        ChangeTimeRange(range) => {
            if model.time_range != range {
                model.time_range = range;
                RefetchData
            } else {
                None
            }
        }
        ToggleMode => {
            model.partner_mode = !model.partner_mode;
            RefetchData
        }
        SetTargetUser(user_id) => {
            model.target_user = user_id;
            if user_id.is_some() && model.partner_mode {
                RefetchData
            } else {
                None
            }
        }
        Pagination(action) => {
            model.pagination.apply(action);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(user_id: u64, secs: i64) -> VoiceLeaderboardEntry {
        VoiceLeaderboardEntry {
            user_id,
            total_duration: secs,
        }
    }

    #[test]
    fn set_entries_empty() {
        let mut model = VoiceLeaderboardModel::new();
        let cmd = voice_leaderboard_update(VoiceLeaderboardMsg::SetEntries(vec![]), &mut model);
        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert!(model.entries.is_empty());
    }

    #[test]
    fn change_time_range_returns_refetch() {
        let mut model = VoiceLeaderboardModel::new();
        let cmd = voice_leaderboard_update(
            VoiceLeaderboardMsg::ChangeTimeRange(VoiceLeaderboardTimeRange::Past7Days),
            &mut model,
        );
        assert_eq!(cmd, VoiceLeaderboardCmd::RefetchData);
        assert_eq!(model.time_range, VoiceLeaderboardTimeRange::Past7Days);
    }

    #[test]
    fn change_time_range_same_returns_none() {
        let mut model = VoiceLeaderboardModel::new();
        let cmd = voice_leaderboard_update(
            VoiceLeaderboardMsg::ChangeTimeRange(VoiceLeaderboardTimeRange::ThisMonth),
            &mut model,
        );
        assert_eq!(cmd, VoiceLeaderboardCmd::None);
    }

    #[test]
    fn toggle_mode() {
        let mut model = VoiceLeaderboardModel::new();
        assert!(!model.partner_mode);

        let cmd = voice_leaderboard_update(VoiceLeaderboardMsg::ToggleMode, &mut model);
        assert_eq!(cmd, VoiceLeaderboardCmd::RefetchData);
        assert!(model.partner_mode);
    }

    #[test]
    fn pagination_first() {
        let mut model = VoiceLeaderboardModel::new();
        model.pagination = PaginationModel::new(5, 3);

        voice_leaderboard_update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::First),
            &mut model,
        );
        assert_eq!(model.pagination.current_page, 1);
    }

    #[test]
    fn pagination_prev() {
        let mut model = VoiceLeaderboardModel::new();
        model.pagination = PaginationModel::new(5, 3);

        voice_leaderboard_update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Prev),
            &mut model,
        );
        assert_eq!(model.pagination.current_page, 2);
    }

    #[test]
    fn pagination_prev_does_not_go_below_one() {
        let mut model = VoiceLeaderboardModel::new();
        model.pagination = PaginationModel::new(5, 1);

        voice_leaderboard_update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Prev),
            &mut model,
        );
        assert_eq!(model.pagination.current_page, 1);
    }

    #[test]
    fn pagination_next() {
        let mut model = VoiceLeaderboardModel::new();
        model.pagination = PaginationModel::new(5, 3);

        voice_leaderboard_update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Next),
            &mut model,
        );
        assert_eq!(model.pagination.current_page, 4);
    }

    #[test]
    fn pagination_next_does_not_exceed_pages() {
        let mut model = VoiceLeaderboardModel::new();
        model.pagination = PaginationModel::new(5, 5);

        voice_leaderboard_update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Next),
            &mut model,
        );
        assert_eq!(model.pagination.current_page, 5);
    }

    #[test]
    fn pagination_last() {
        let mut model = VoiceLeaderboardModel::new();
        model.pagination = PaginationModel::new(5, 3);

        voice_leaderboard_update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Last),
            &mut model,
        );
        assert_eq!(model.pagination.current_page, 5);
    }

    #[test]
    fn pagination_page_is_no_op() {
        let mut model = VoiceLeaderboardModel::new();
        model.pagination = PaginationModel::new(5, 3);

        voice_leaderboard_update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Page),
            &mut model,
        );
        assert_eq!(model.pagination.current_page, 3);
    }

    #[test]
    fn target_is_author() {
        let model = VoiceLeaderboardModel::new();
        assert!(model.target_user.is_none());
    }

    #[test]
    fn set_target_user_clear() {
        let mut model = VoiceLeaderboardModel::new();
        model.target_user = Some(42);

        let cmd = voice_leaderboard_update(VoiceLeaderboardMsg::SetTargetUser(None), &mut model);

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert!(model.target_user.is_none());
    }

    #[test]
    fn set_target_user_in_partner_mode_refetches() {
        let mut model = VoiceLeaderboardModel::new();
        model.partner_mode = true;

        let cmd =
            voice_leaderboard_update(VoiceLeaderboardMsg::SetTargetUser(Some(42)), &mut model);

        assert_eq!(cmd, VoiceLeaderboardCmd::RefetchData);
        assert_eq!(model.target_user, Some(42));
    }

    #[test]
    fn set_target_user_not_partner_mode_no_refetch() {
        let mut model = VoiceLeaderboardModel::new();
        model.partner_mode = false;

        let cmd =
            voice_leaderboard_update(VoiceLeaderboardMsg::SetTargetUser(Some(42)), &mut model);

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert_eq!(model.target_user, Some(42));
    }

    #[test]
    fn current_page_rank_offset() {
        let mut model = VoiceLeaderboardModel::new();
        model.entries = vec![
            make_entry(1, 100),
            make_entry(2, 90),
            make_entry(3, 80),
            make_entry(4, 70),
            make_entry(5, 60),
        ];
        model.pagination = PaginationModel::new(1, 1);

        assert_eq!(model.entries.len(), 5);
    }
}
