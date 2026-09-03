//! Pure update logic for the voice leaderboard.
//!
//! All business logic — pagination, mode toggling, time-range changes,
//! and entry bookkeeping — lives here so it can be unit-tested without
//! touching Discord or the database. The model is the single source of truth
//! for the leaderboard view: it absorbs the entry list, the user rank, the
//! time range, the partner/server mode, the target user, the page image
//! bytes, and the pagination state ([`PaginationModel`]).

use crate::bot::command::voice::VoiceLeaderboardTimeRange;
use crate::entity::VoiceLeaderboardEntry;
use crate::update::pagination::PaginationAction;
use crate::update::pagination::PaginationModel;

/// Messages that can mutate the leaderboard model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceLeaderboardMsg {
    /// Boot handshake — the host dispatches this on start.
    Start,
    /// The view loop timed out.
    Expired,
    /// Replace the full entry set after a database fetch. Carries the resolved
    /// partner display name (used only in partner mode).
    EntriesLoaded(LeaderboardData),
    /// Change the active time range.
    ChangeTimeRange(VoiceLeaderboardTimeRange),
    /// Toggle between server-wide and partner mode.
    ToggleMode,
    /// Set (or clear) the target user for partner mode.
    SetTargetUser(Option<u64>),
    /// Navigate pagination.
    Pagination(PaginationAction),
    /// The adapter finished (re)rendering the current page image.
    ImageRendered(Option<Vec<u8>>),
}

/// Data returned by a leaderboard query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderboardData {
    /// The full ranked entry list for the current filters.
    pub entries: Vec<VoiceLeaderboardEntry>,
    /// The resolved partner display name, if the query was for a partner.
    pub target_user_name: Option<String>,
}

/// Effects the leaderboard view can request.
///
/// Data-only. In-session data fetching and page-image rendering are both
/// driven through [`VoiceLeaderboardEffect`], executed by the shell adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceLeaderboardEffect {
    /// Refetch the leaderboard entries for the current filters.
    QueryLeaderboard {
        /// The active time range.
        time_range: VoiceLeaderboardTimeRange,
        /// Whether the leaderboard is in partner mode.
        is_partner_mode: bool,
        /// The target user id (partner mode only).
        target_user_id: Option<u64>,
    },
    /// (Re)render the current page image from the given page slice.
    RenderImage {
        /// The entries visible on the current page.
        entries: Vec<VoiceLeaderboardEntry>,
        /// The rank offset of the first entry on the page.
        rank_offset: u32,
    },
}

/// The leaderboard model — everything needed to render a page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VoiceLeaderboardModel {
    pub(crate) entries: Vec<VoiceLeaderboardEntry>,
    pub(crate) user_rank: Option<u32>,
    pub(crate) user_duration: Option<i64>,
    pub(crate) time_range: VoiceLeaderboardTimeRange,
    pub(crate) is_partner_mode: bool,
    pub(crate) target_user_id: Option<u64>,
    pub(crate) target_user_name: Option<String>,
    pub(crate) author_id: u64,
    pub(crate) pagination: PaginationModel,
    pub(crate) image_bytes: Option<Vec<u8>>,
    pub(crate) pagination_disabled: bool,
}

impl VoiceLeaderboardModel {
    /// Creates a model from raw entries.
    pub fn from_entries(
        entries: Vec<VoiceLeaderboardEntry>,
        author_id: u64,
        per_page: u32,
    ) -> Self {
        let mut model = Self {
            pagination: PaginationModel {
                per_page: per_page.max(1),
                ..PaginationModel::default()
            },
            author_id,
            ..Self::default()
        };
        model.apply_entries(entries);
        model
    }

    fn apply_entries(&mut self, entries: Vec<VoiceLeaderboardEntry>) {
        self.user_rank = entries
            .iter()
            .position(|e| e.user_id == self.author_id)
            .map(|p| p as u32 + 1);
        self.user_duration = entries
            .iter()
            .find(|e| e.user_id == self.author_id)
            .map(|e| e.total_duration);
        self.entries = entries;
        self.recompute_pagination();
    }

    /// Recomputes the page count and clamps the current page after entries change.
    fn recompute_pagination(&mut self) {
        let pages = self.pages();
        self.pagination.pages = pages;
        self.pagination.current_page = self.pagination.current_page.clamp(1, pages);
    }

    /// Total number of pages.
    pub fn pages(&self) -> u32 {
        if self.entries.is_empty() {
            1
        } else {
            (self.entries.len() as u32)
                .div_ceil(self.pagination.per_page)
                .max(1)
        }
    }

    /// The current pagination state (page, pages, per-page).
    pub fn pagination(&self) -> &PaginationModel {
        &self.pagination
    }

    /// The currently displayed page number (1-based).
    pub fn current_page(&self) -> u32 {
        self.pagination.current_page
    }

    /// The number of entries per page.
    pub fn per_page(&self) -> u32 {
        self.pagination.per_page
    }

    /// Whether the current page image has been rendered.
    pub fn image_bytes(&self) -> Option<&[u8]> {
        self.image_bytes.as_deref()
    }

    /// Whether the pagination row is disabled (e.g. after a timeout).
    pub fn pagination_disabled(&self) -> bool {
        self.pagination_disabled
    }

    /// The resolved partner display name, if any.
    pub fn target_user_name(&self) -> Option<&str> {
        self.target_user_name.as_deref()
    }

    /// The target user id, if one is set (partner mode only).
    pub fn target_user_id(&self) -> Option<u64> {
        self.target_user_id
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
        let offset =
            ((self.pagination.current_page.saturating_sub(1)) * self.pagination.per_page) as usize;
        let end = (offset + self.pagination.per_page as usize).min(self.entries.len());
        (offset, end)
    }

    /// Returns the rank offset for the current page.
    pub fn current_page_rank_offset(&self) -> u32 {
        (self.pagination.current_page.saturating_sub(1)) * self.pagination.per_page
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

    /// Whether the leaderboard is in partner mode.
    pub fn is_partner_mode(&self) -> bool {
        self.is_partner_mode
    }

    /// The active time range.
    pub fn time_range(&self) -> VoiceLeaderboardTimeRange {
        self.time_range
    }

    /// The author's rank, if present in the current entries.
    pub fn user_rank(&self) -> Option<u32> {
        self.user_rank
    }

    /// The author's total duration, if present in the current entries.
    pub fn user_duration(&self) -> Option<i64> {
        self.user_duration
    }

    /// The author's user id.
    pub fn author_id(&self) -> u64 {
        self.author_id
    }

    /// The full ranked entry list.
    pub fn entries(&self) -> &[VoiceLeaderboardEntry] {
        &self.entries
    }

    /// Sets the rendered page image bytes (used to seed the model at boot).
    pub fn with_image_bytes(mut self, image_bytes: Option<Vec<u8>>) -> Self {
        self.image_bytes = image_bytes;
        self
    }
}

/// The pure update function — the only writer of the leaderboard model.
///
/// `Start` is a no-op (initial data arrives via the feature config). Time-range,
/// mode, and target-user changes return a [`VoiceLeaderboardEffect::QueryLeaderboard`]
/// so the adapter refetches entries; the returned [`VoiceLeaderboardMsg::EntriesLoaded`]
/// replaces the entry set and requests a fresh page image. Pagination is pure
/// (the full list is loaded upfront) and requests a [`VoiceLeaderboardEffect::RenderImage`]
/// directly.
pub fn update(
    msg: VoiceLeaderboardMsg,
    model: &mut VoiceLeaderboardModel,
) -> Vec<VoiceLeaderboardEffect> {
    use VoiceLeaderboardMsg::*;

    match msg {
        Start => Vec::new(),
        Expired => {
            model.pagination_disabled = true;
            Vec::new()
        }
        EntriesLoaded(data) => {
            model.target_user_name = data.target_user_name;
            model.apply_entries(data.entries);
            vec![render_image(model)]
        }
        ChangeTimeRange(range) => {
            if model.time_range != range {
                model.time_range = range;
                vec![query(model)]
            } else {
                Vec::new()
            }
        }
        ToggleMode => {
            model.is_partner_mode = !model.is_partner_mode;
            vec![query(model)]
        }
        SetTargetUser(user_id) => {
            model.target_user_id = user_id;
            if model.is_partner_mode {
                vec![query(model)]
            } else {
                Vec::new()
            }
        }
        Pagination(action) => {
            model.pagination.apply(action);
            vec![render_image(model)]
        }
        ImageRendered(bytes) => {
            model.image_bytes = bytes;
            Vec::new()
        }
    }
}

/// Builds a [`VoiceLeaderboardEffect::QueryLeaderboard`] from the current model.
fn query(model: &VoiceLeaderboardModel) -> VoiceLeaderboardEffect {
    VoiceLeaderboardEffect::QueryLeaderboard {
        time_range: model.time_range,
        is_partner_mode: model.is_partner_mode,
        target_user_id: model.target_user_id,
    }
}

/// Builds a [`VoiceLeaderboardEffect::RenderImage`] for the current page.
fn render_image(model: &VoiceLeaderboardModel) -> VoiceLeaderboardEffect {
    VoiceLeaderboardEffect::RenderImage {
        entries: model.current_page_entries().to_vec(),
        rank_offset: model.current_page_rank_offset(),
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

    // ── EntriesLoaded (SetEntries) ─────────────────────────────────────────

    #[test]
    fn entries_loaded_computes_rank_and_duration() {
        let mut model = VoiceLeaderboardModel {
            author_id: 200,
            pagination: PaginationModel {
                per_page: 10,
                ..PaginationModel::default()
            },
            ..VoiceLeaderboardModel::default()
        };

        let effects = update(
            VoiceLeaderboardMsg::EntriesLoaded(LeaderboardData {
                entries: vec![entry(100, 3600), entry(200, 1800), entry(300, 900)],
                target_user_name: None,
            }),
            &mut model,
        );

        assert_eq!(effects.len(), 1);
        assert!(matches!(
            &effects[0],
            VoiceLeaderboardEffect::RenderImage { .. }
        ));
        assert_eq!(model.entries.len(), 3);
        assert_eq!(model.user_rank, Some(2));
        assert_eq!(model.user_duration, Some(1800));
        assert_eq!(model.pages(), 1);
        assert_eq!(model.current_page(), 1);
    }

    #[test]
    fn entries_loaded_author_not_in_list() {
        let mut model = model_with(vec![entry(1, 100), entry(2, 200)], 10);
        model.author_id = 999;

        update(
            VoiceLeaderboardMsg::EntriesLoaded(LeaderboardData {
                entries: vec![entry(1, 100), entry(2, 200)],
                target_user_name: None,
            }),
            &mut model,
        );

        assert_eq!(model.user_rank, None);
        assert_eq!(model.user_duration, None);
    }

    #[test]
    fn entries_loaded_clamps_page() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.pagination.current_page = 5; // out of bounds

        update(
            VoiceLeaderboardMsg::EntriesLoaded(LeaderboardData {
                entries: vec![entry(1, 100); 5],
                target_user_name: None,
            }),
            &mut model,
        );

        assert_eq!(model.pages(), 1);
        assert_eq!(model.current_page(), 1);
    }

    #[test]
    fn entries_loaded_empty() {
        let mut model = model_with(vec![entry(1, 100)], 10);

        update(
            VoiceLeaderboardMsg::EntriesLoaded(LeaderboardData {
                entries: vec![],
                target_user_name: None,
            }),
            &mut model,
        );

        assert!(model.is_empty());
        assert_eq!(model.pages(), 1);
        assert_eq!(model.current_page(), 1);
    }

    #[test]
    fn entries_loaded_updates_target_user_name() {
        let mut model = model_with(vec![], 10);
        model.is_partner_mode = true;
        model.target_user_id = Some(42);

        update(
            VoiceLeaderboardMsg::EntriesLoaded(LeaderboardData {
                entries: vec![],
                target_user_name: Some("partner".to_string()),
            }),
            &mut model,
        );

        assert_eq!(model.target_user_name(), Some("partner"));
    }

    // ── ChangeTimeRange ─────────────────────────────────────────────────────

    #[test]
    fn change_time_range_returns_query() {
        let mut model = model_with(vec![], 10);
        model.time_range = VoiceLeaderboardTimeRange::ThisMonth;

        let effects = update(
            VoiceLeaderboardMsg::ChangeTimeRange(VoiceLeaderboardTimeRange::Past7Days),
            &mut model,
        );

        assert_eq!(
            effects,
            vec![VoiceLeaderboardEffect::QueryLeaderboard {
                time_range: VoiceLeaderboardTimeRange::Past7Days,
                is_partner_mode: false,
                target_user_id: None,
            }]
        );
        assert_eq!(model.time_range, VoiceLeaderboardTimeRange::Past7Days);
    }

    #[test]
    fn change_time_range_same_is_noop() {
        let mut model = model_with(vec![], 10);
        model.time_range = VoiceLeaderboardTimeRange::ThisMonth;

        let effects = update(
            VoiceLeaderboardMsg::ChangeTimeRange(VoiceLeaderboardTimeRange::ThisMonth),
            &mut model,
        );

        assert!(effects.is_empty());
    }

    // ── ToggleMode ──────────────────────────────────────────────────────────

    #[test]
    fn toggle_mode_returns_query() {
        let mut model = model_with(vec![], 10);
        assert!(!model.is_partner_mode);

        let effects = update(VoiceLeaderboardMsg::ToggleMode, &mut model);

        assert_eq!(effects.len(), 1);
        assert!(model.is_partner_mode);

        let effects = update(VoiceLeaderboardMsg::ToggleMode, &mut model);

        assert_eq!(effects.len(), 1);
        assert!(!model.is_partner_mode);
    }

    // ── SetTargetUser ───────────────────────────────────────────────────────

    #[test]
    fn set_target_user_in_partner_mode_queries() {
        let mut model = model_with(vec![], 10);
        model.is_partner_mode = true;

        let effects = update(VoiceLeaderboardMsg::SetTargetUser(Some(42)), &mut model);

        assert_eq!(effects.len(), 1);
        assert!(matches!(
            &effects[0],
            VoiceLeaderboardEffect::QueryLeaderboard {
                target_user_id: Some(42),
                ..
            }
        ));
        assert_eq!(model.target_user_id, Some(42));
    }

    #[test]
    fn set_target_user_not_partner_mode_no_query() {
        let mut model = model_with(vec![], 10);
        model.is_partner_mode = false;

        let effects = update(VoiceLeaderboardMsg::SetTargetUser(Some(42)), &mut model);

        assert!(effects.is_empty());
        assert_eq!(model.target_user_id, Some(42));
    }

    #[test]
    fn set_target_user_clear() {
        let mut model = model_with(vec![], 10);
        model.target_user_id = Some(42);

        let effects = update(VoiceLeaderboardMsg::SetTargetUser(None), &mut model);

        assert!(effects.is_empty());
        assert_eq!(model.target_user_id, None);
    }

    // ── Pagination ──────────────────────────────────────────────────────────

    #[test]
    fn pagination_first() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.pagination.current_page = 3;

        let effects = update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::First),
            &mut model,
        );

        assert_eq!(effects.len(), 1);
        assert!(matches!(
            &effects[0],
            VoiceLeaderboardEffect::RenderImage { .. }
        ));
        assert_eq!(model.current_page(), 1);
    }

    #[test]
    fn pagination_prev() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.pagination.current_page = 2;

        update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Prev),
            &mut model,
        );

        assert_eq!(model.current_page(), 1);
    }

    #[test]
    fn pagination_prev_does_not_go_below_one() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.pagination.current_page = 1;

        update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Prev),
            &mut model,
        );

        assert_eq!(model.current_page(), 1);
    }

    #[test]
    fn pagination_next() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.pagination.current_page = 1;

        update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Next),
            &mut model,
        );

        assert_eq!(model.current_page(), 2);
    }

    #[test]
    fn pagination_next_does_not_exceed_pages() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.pagination.current_page = 3; // last page

        update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Next),
            &mut model,
        );

        assert_eq!(model.current_page(), 3);
    }

    #[test]
    fn pagination_last() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.pagination.current_page = 1;

        update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Last),
            &mut model,
        );

        assert_eq!(model.current_page(), 3);
    }

    #[test]
    fn pagination_page_is_no_op() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.pagination.current_page = 2;

        update(
            VoiceLeaderboardMsg::Pagination(PaginationAction::Page),
            &mut model,
        );

        assert_eq!(model.current_page(), 2);
    }

    // ── Model helpers ───────────────────────────────────────────────────────

    #[test]
    fn current_page_indices() {
        let model = model_with(vec![entry(1, 100); 25], 10);
        assert_eq!(model.current_page_indices(), (0, 10));

        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.pagination.current_page = 2;
        assert_eq!(model.current_page_indices(), (10, 20));

        let mut model = model_with(vec![entry(1, 100); 25], 10);
        model.pagination.current_page = 3;
        assert_eq!(model.current_page_indices(), (20, 25));
    }

    #[test]
    fn current_page_indices_empty() {
        let model = model_with(vec![], 10);
        assert_eq!(model.current_page_indices(), (0, 0));
    }

    #[test]
    fn current_page_rank_offset() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        assert_eq!(model.current_page_rank_offset(), 0);

        model.pagination.current_page = 2;
        assert_eq!(model.current_page_rank_offset(), 10);

        model.pagination.current_page = 3;
        assert_eq!(model.current_page_rank_offset(), 20);
    }

    #[test]
    fn current_page_entries() {
        let entries: Vec<_> = (1..=25).map(|i| entry(i, i as i64 * 100)).collect();
        let mut model = model_with(entries.clone(), 10);

        assert_eq!(model.current_page_entries().len(), 10);
        assert_eq!(model.current_page_entries()[0].user_id, 1);

        model.pagination.current_page = 2;
        assert_eq!(model.current_page_entries().len(), 10);
        assert_eq!(model.current_page_entries()[0].user_id, 11);

        model.pagination.current_page = 3;
        assert_eq!(model.current_page_entries().len(), 5);
        assert_eq!(model.current_page_entries()[0].user_id, 21);
    }

    #[test]
    fn target_is_author() {
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
    fn pages_calculation() {
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
    fn pages_empty() {
        let model = model_with(vec![], 10);
        assert_eq!(model.pages(), 1);
    }

    #[test]
    fn from_entries() {
        let entries = vec![entry(100, 3600), entry(200, 1800), entry(300, 900)];
        let model = VoiceLeaderboardModel::from_entries(entries.clone(), 200, 10);

        assert_eq!(model.entries, entries);
        assert_eq!(model.author_id, 200);
        assert_eq!(model.user_rank, Some(2));
        assert_eq!(model.user_duration, Some(1800));
        assert_eq!(model.per_page(), 10);
        assert_eq!(model.current_page(), 1);
    }

    #[test]
    fn expired_disables_pagination() {
        let mut model = model_with(vec![entry(1, 100); 25], 10);
        let effects = update(VoiceLeaderboardMsg::Expired, &mut model);
        assert!(effects.is_empty());
        assert!(model.pagination_disabled());
    }

    #[test]
    fn start_is_noop() {
        let mut model = model_with(vec![], 10);
        let effects = update(VoiceLeaderboardMsg::Start, &mut model);
        assert!(effects.is_empty());
    }

    #[test]
    fn image_rendered_sets_bytes() {
        let mut model = model_with(vec![], 10);
        let effects = update(
            VoiceLeaderboardMsg::ImageRendered(Some(vec![1, 2, 3])),
            &mut model,
        );
        assert!(effects.is_empty());
        assert_eq!(model.image_bytes(), Some(&[1, 2, 3][..]));
    }
}
