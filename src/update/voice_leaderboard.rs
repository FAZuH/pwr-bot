//! Pure update logic for the voice leaderboard.
//!
//! All business logic — pagination, mode toggling, time-range changes,
//! and entry bookkeeping — lives here so it can be unit-tested without
//! touching Discord or the database.

use crate::bot::command::voice::VoiceLeaderboardTimeRange;
use crate::bot::view::pagination::PaginationAction;
use crate::entity::VoiceLeaderboardEntry;
use crate::update::Update;

/// Messages that can mutate the leaderboard model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceLeaderboardMsg {
    /// Replace the full entry set (e.g. after a database fetch).
    SetEntries(Vec<VoiceLeaderboardEntry>),
    /// Change the active time range.
    ChangeTimeRange(VoiceLeaderboardTimeRange),
    /// Toggle between server-wide and partner mode.
    ToggleMode,
    /// Set (or clear) the target user for partner mode.
    SetTargetUser(Option<u64>),
    /// Navigate pagination.
    Pagination(PaginationAction),
}

/// Commands returned by the update, instructing the caller what to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceLeaderboardCmd {
    /// Nothing else to do.
    None,
    /// The caller should re-fetch leaderboard entries from the database.
    RefetchData,
}

/// The leaderboard model — everything needed to render a page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VoiceLeaderboardModel {
    pub entries: Vec<VoiceLeaderboardEntry>,
    pub user_rank: Option<u32>,
    pub user_duration: Option<i64>,
    pub time_range: VoiceLeaderboardTimeRange,
    pub is_partner_mode: bool,
    pub target_user_id: Option<u64>,
    pub author_id: u64,
    pub current_page: u32,
    pub per_page: u32,
}

impl VoiceLeaderboardModel {
    /// Creates a model from raw entries.
    pub fn from_entries(
        entries: Vec<VoiceLeaderboardEntry>,
        author_id: u64,
        per_page: u32,
    ) -> Self {
        let mut model = Self {
            per_page: per_page.max(1),
            author_id,
            ..Self::default()
        };
        Self::apply_entries(&mut model, entries);
        model
    }

    fn apply_entries(model: &mut Self, entries: Vec<VoiceLeaderboardEntry>) {
        model.user_rank = entries
            .iter()
            .position(|e| e.user_id == model.author_id)
            .map(|p| p as u32 + 1);
        model.user_duration = entries
            .iter()
            .find(|e| e.user_id == model.author_id)
            .map(|e| e.total_duration);
        model.entries = entries;
        model.pages();
        model.current_page = model.current_page.clamp(1, model.pages().max(1));
    }

    /// Total number of pages.
    pub fn pages(&self) -> u32 {
        if self.entries.is_empty() {
            1
        } else {
            (self.entries.len() as u32).div_ceil(self.per_page).max(1)
        }
    }

    /// Whether there is any data to show.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Calculates the slice indices for the current page.
    pub fn current_page_indices(&self) -> (usize, usize) {
        if self.entries.is_empty() {
            return (0, 0);
        }
        let offset = ((self.current_page.saturating_sub(1)) * self.per_page) as usize;
        let end = (offset + self.per_page as usize).min(self.entries.len());
        (offset, end)
    }

    /// Returns the rank offset for the current page.
    pub fn current_page_rank_offset(&self) -> u32 {
        (self.current_page.saturating_sub(1)) * self.per_page
    }

    /// Entries visible on the current page.
    pub fn current_page_entries(&self) -> &[VoiceLeaderboardEntry] {
        let (start, end) = self.current_page_indices();
        &self.entries[start..end]
    }

    /// Whether the target user is the author.
    pub fn target_is_author(&self) -> bool {
        self.target_user_id == Some(self.author_id)
    }
}

/// The update implementation for voice leaderboard.
#[derive(Debug, Clone, Copy, Default)]
pub struct VoiceLeaderboardUpdate;

impl VoiceLeaderboardUpdate {
    pub fn new() -> Self {
        Self
    }
}

impl Update for VoiceLeaderboardUpdate {
    type Model = VoiceLeaderboardModel;
    type Msg = VoiceLeaderboardMsg;
    type Cmd = VoiceLeaderboardCmd;

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Self::Cmd {
        use VoiceLeaderboardCmd::*;
        use VoiceLeaderboardMsg::*;

        match msg {
            SetEntries(entries) => {
                VoiceLeaderboardModel::apply_entries(model, entries);
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
                model.is_partner_mode = !model.is_partner_mode;
                RefetchData
            }
            SetTargetUser(user_id) => {
                model.target_user_id = user_id;
                if model.is_partner_mode {
                    RefetchData
                } else {
                    None
                }
            }
            Pagination(action) => {
                let pages = model.pages();
                match action {
                    PaginationAction::First => model.current_page = 1,
                    PaginationAction::Prev => {
                        model.current_page = model.current_page.saturating_sub(1).max(1);
                    }
                    PaginationAction::Next => {
                        model.current_page = (model.current_page + 1).min(pages);
                    }
                    PaginationAction::Last => model.current_page = pages,
                    PaginationAction::Page => {}
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(user_id: u64, duration: i64) -> VoiceLeaderboardEntry {
        VoiceLeaderboardEntry {
            user_id,
            total_duration: duration,
        }
    }

    fn model_with(entries: Vec<VoiceLeaderboardEntry>, per_page: u32) -> VoiceLeaderboardModel {
        VoiceLeaderboardModel::from_entries(entries, 100, per_page)
    }

    // ── SetEntries ──────────────────────────────────────────────────────────

    #[test]
    fn test_set_entries_computes_rank_and_duration() {
        let mut model = VoiceLeaderboardModel {
            author_id: 200,
            per_page: 10,
            ..VoiceLeaderboardModel::default()
        };

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::SetEntries(vec![
                entry(100, 3600),
                entry(200, 1800),
                entry(300, 900),
            ]),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert_eq!(model.entries.len(), 3);
        assert_eq!(model.user_rank, Some(2));
        assert_eq!(model.user_duration, Some(1800));
        assert_eq!(model.pages(), 1);
        assert_eq!(model.current_page, 1);
    }

    #[test]
    fn test_set_entries_author_not_in_list() {
        let mut model = model_with(vec![entry(1, 100), entry(2, 200)], 10);
        model.author_id = 999;

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::SetEntries(vec![entry(1, 100), entry(2, 200)]),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert_eq!(model.user_rank, None);
        assert_eq!(model.user_duration, None);
    }

    #[test]
    fn test_set_entries_clamps_page() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.current_page = 5; // out of bounds

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::SetEntries(vec![entry(1, 100); 5]),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert_eq!(model.pages(), 1);
        assert_eq!(model.current_page, 1);
    }

    #[test]
    fn test_set_entries_empty() {
        let mut model = model_with(vec![entry(1, 100)], 10);

        let cmd =
            VoiceLeaderboardUpdate::update(VoiceLeaderboardMsg::SetEntries(vec![]), &mut model);

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert!(model.is_empty());
        assert_eq!(model.pages(), 1);
        assert_eq!(model.current_page, 1);
    }

    // ── ChangeTimeRange ─────────────────────────────────────────────────────

    #[test]
    fn test_change_time_range_returns_refetch() {
        let mut model = model_with(vec![], 10);
        model.time_range = VoiceLeaderboardTimeRange::ThisMonth;

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::ChangeTimeRange(VoiceLeaderboardTimeRange::Past7Days),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::RefetchData);
        assert_eq!(model.time_range, VoiceLeaderboardTimeRange::Past7Days);
    }

    #[test]
    fn test_change_time_range_same_returns_none() {
        let mut model = model_with(vec![], 10);
        model.time_range = VoiceLeaderboardTimeRange::ThisMonth;

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::ChangeTimeRange(VoiceLeaderboardTimeRange::ThisMonth),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
    }

    // ── ToggleMode ──────────────────────────────────────────────────────────

    #[test]
    fn test_toggle_mode() {
        let mut model = model_with(vec![], 10);
        assert!(!model.is_partner_mode);

        let cmd = VoiceLeaderboardUpdate::update(VoiceLeaderboardMsg::ToggleMode, &mut model);

        assert_eq!(cmd, VoiceLeaderboardCmd::RefetchData);
        assert!(model.is_partner_mode);

        let cmd = VoiceLeaderboardUpdate::update(VoiceLeaderboardMsg::ToggleMode, &mut model);

        assert_eq!(cmd, VoiceLeaderboardCmd::RefetchData);
        assert!(!model.is_partner_mode);
    }

    // ── SetTargetUser ───────────────────────────────────────────────────────

    #[test]
    fn test_set_target_user_in_partner_mode_refetches() {
        let mut model = model_with(vec![], 10);
        model.is_partner_mode = true;

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::SetTargetUser(Some(42)),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::RefetchData);
        assert_eq!(model.target_user_id, Some(42));
    }

    #[test]
    fn test_set_target_user_not_partner_mode_no_refetch() {
        let mut model = model_with(vec![], 10);
        model.is_partner_mode = false;

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::SetTargetUser(Some(42)),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert_eq!(model.target_user_id, Some(42));
    }

    #[test]
    fn test_set_target_user_clear() {
        let mut model = model_with(vec![], 10);
        model.target_user_id = Some(42);

        let cmd =
            VoiceLeaderboardUpdate::update(VoiceLeaderboardMsg::SetTargetUser(None), &mut model);

        assert_eq!(model.target_user_id, None);
        assert_eq!(cmd, VoiceLeaderboardCmd::None);
    }

    // ── Pagination ──────────────────────────────────────────────────────────

    #[test]
    fn test_pagination_first() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.current_page = 3;

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::First),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert_eq!(model.current_page, 1);
    }

    #[test]
    fn test_pagination_prev() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.current_page = 2;

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Prev),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert_eq!(model.current_page, 1);
    }

    #[test]
    fn test_pagination_prev_does_not_go_below_one() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.current_page = 1;

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Prev),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert_eq!(model.current_page, 1);
    }

    #[test]
    fn test_pagination_next() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.current_page = 1;

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Next),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert_eq!(model.current_page, 2);
    }

    #[test]
    fn test_pagination_next_does_not_exceed_pages() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.current_page = 3; // last page

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Next),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert_eq!(model.current_page, 3);
    }

    #[test]
    fn test_pagination_last() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.current_page = 1;

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Last),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert_eq!(model.current_page, 3);
    }

    #[test]
    fn test_pagination_page_is_no_op() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.current_page = 2;

        let cmd = VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Page),
            &mut model,
        );

        assert_eq!(cmd, VoiceLeaderboardCmd::None);
        assert_eq!(model.current_page, 2);
    }

    // ── Model helpers ───────────────────────────────────────────────────────

    #[test]
    fn test_current_page_indices() {
        let model = model_with(vec![entry(1, 100); 25], 10);
        assert_eq!(model.current_page_indices(), (0, 10));

        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.current_page = 2;
        assert_eq!(model.current_page_indices(), (10, 20));

        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.current_page = 3;
        assert_eq!(model.current_page_indices(), (20, 25));
    }

    #[test]
    fn test_current_page_indices_empty() {
        let model = model_with(vec![], 10);
        assert_eq!(model.current_page_indices(), (0, 0));
    }

    #[test]
    fn test_current_page_rank_offset() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        assert_eq!(model.current_page_rank_offset(), 0);

        model.current_page = 2;
        assert_eq!(model.current_page_rank_offset(), 10);

        model.current_page = 3;
        assert_eq!(model.current_page_rank_offset(), 20);
    }

    #[test]
    fn test_current_page_entries() {
        let entries: Vec<_> = (1..=25).map(|i| entry(i, i as i64 * 100)).collect();
        let mut model = model_with(entries.clone(), 10);

        assert_eq!(model.current_page_entries().len(), 10);
        assert_eq!(model.current_page_entries()[0].user_id, 1);

        model.current_page = 2;
        assert_eq!(model.current_page_entries().len(), 10);
        assert_eq!(model.current_page_entries()[0].user_id, 11);

        model.current_page = 3;
        assert_eq!(model.current_page_entries().len(), 5);
        assert_eq!(model.current_page_entries()[0].user_id, 21);
    }

    #[test]
    fn test_target_is_author() {
        let mut model = model_with(vec![], 10);
        model.author_id = 100;
        model.target_user_id = Some(100);
        assert!(model.target_is_author());

        model.target_user_id = Some(200);
        assert!(!model.target_is_author());

        model.target_user_id = None;
        assert!(!model.target_is_author());
    }

    #[test]
    fn test_pages_calculation() {
        let model = model_with(vec![entry(1, 100); 5], 10);
        assert_eq!(model.pages(), 1);

        let model = model_with(vec![entry(1, 100); 10], 10);
        assert_eq!(model.pages(), 1);

        let model = model_with(vec![entry(1, 100); 11], 10);
        assert_eq!(model.pages(), 2);

        let model = model_with(vec![entry(1, 100); 25], 10);
        assert_eq!(model.pages(), 3);
    }

    #[test]
    fn test_pages_empty() {
        let model = model_with(vec![], 10);
        assert_eq!(model.pages(), 1);
    }

    #[test]
    fn test_from_entries() {
        let entries = vec![entry(100, 3600), entry(200, 1800), entry(300, 900)];
        let model = VoiceLeaderboardModel::from_entries(entries.clone(), 200, 10);

        assert_eq!(model.entries, entries);
        assert_eq!(model.author_id, 200);
        assert_eq!(model.user_rank, Some(2));
        assert_eq!(model.user_duration, Some(1800));
        assert_eq!(model.per_page, 10);
        assert_eq!(model.current_page, 1);
    }
}
