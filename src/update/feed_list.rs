//! Pure update logic for the feed subscription list.
//!
//! Manages view/edit state, mark-for-unsubscribe bookkeeping, and pagination.

use std::collections::HashSet;

use crate::bot::view::pagination::PaginationAction;
use crate::update::Update;

/// View state for the feed list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedListViewState {
    View,
    Edit,
}

/// Messages that can mutate the feed-list model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedListMsg {
    /// Switch to edit mode.
    Edit,
    /// Switch to view mode.
    View,
    /// Toggle a subscription's mark-for-removal status.
    ToggleUnsub { source_url: String },
    /// Save removals and return to view mode.
    Save,
    /// Navigate pagination.
    Pagination(PaginationAction),
}

/// Commands returned by the update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedListCmd {
    /// Nothing else to do.
    None,
    /// Perform actual unsubscriptions and refetch the list.
    SaveUnsubscribes(HashSet<String>),
    /// Refetch subscriptions for the current page.
    RefetchSubscriptions,
}

/// The feed-list model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedListModel {
    pub state: FeedListViewState,
    pub marked_unsub: HashSet<String>,
    pub current_page: u32,
    pub per_page: u32,
    pub pagination_disabled: bool,
}

impl FeedListModel {
    pub fn new(per_page: u32) -> Self {
        Self {
            state: FeedListViewState::View,
            marked_unsub: HashSet::new(),
            current_page: 1,
            per_page: per_page.max(1),
            pagination_disabled: false,
        }
    }
}

/// The update implementation for feed list.
#[derive(Debug, Clone, Copy, Default)]
pub struct FeedListUpdate;

impl FeedListUpdate {
    pub fn new() -> Self {
        Self
    }
}

impl Update for FeedListUpdate {
    type Model = FeedListModel;
    type Msg = FeedListMsg;
    type Cmd = FeedListCmd;

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Self::Cmd {
        use FeedListMsg::*;

        match msg {
            Edit => {
                model.state = FeedListViewState::Edit;
                FeedListCmd::None
            }
            View => {
                model.state = FeedListViewState::View;
                FeedListCmd::None
            }
            ToggleUnsub { source_url } => {
                if model.marked_unsub.contains(&source_url) {
                    model.marked_unsub.remove(&source_url);
                } else {
                    model.marked_unsub.insert(source_url);
                }
                FeedListCmd::None
            }
            Save => {
                let to_remove: HashSet<String> = model.marked_unsub.drain().collect();
                model.state = FeedListViewState::View;
                if to_remove.is_empty() {
                    FeedListCmd::RefetchSubscriptions
                } else {
                    FeedListCmd::SaveUnsubscribes(to_remove)
                }
            }
            Pagination(action) => {
                match action {
                    PaginationAction::First => model.current_page = 1,
                    PaginationAction::Prev => {
                        model.current_page = model.current_page.saturating_sub(1).max(1);
                    }
                    PaginationAction::Next => {}
                    PaginationAction::Last => {}
                    PaginationAction::Page => {}
                }
                FeedListCmd::RefetchSubscriptions
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_sets_state() {
        let mut model = FeedListModel::new(10);
        assert_eq!(model.state, FeedListViewState::View);

        let cmd = FeedListUpdate::update(FeedListMsg::Edit, &mut model);

        assert_eq!(cmd, FeedListCmd::None);
        assert_eq!(model.state, FeedListViewState::Edit);
    }

    #[test]
    fn test_view_sets_state() {
        let mut model = FeedListModel::new(10);
        model.state = FeedListViewState::Edit;

        let cmd = FeedListUpdate::update(FeedListMsg::View, &mut model);

        assert_eq!(cmd, FeedListCmd::None);
        assert_eq!(model.state, FeedListViewState::View);
    }

    #[test]
    fn test_toggle_unsub_adds() {
        let mut model = FeedListModel::new(10);

        let cmd = FeedListUpdate::update(
            FeedListMsg::ToggleUnsub {
                source_url: "https://example.com".to_string(),
            },
            &mut model,
        );

        assert_eq!(cmd, FeedListCmd::None);
        assert!(model.marked_unsub.contains("https://example.com"));
    }

    #[test]
    fn test_toggle_unsub_removes() {
        let mut model = FeedListModel::new(10);
        model.marked_unsub.insert("https://example.com".to_string());

        let cmd = FeedListUpdate::update(
            FeedListMsg::ToggleUnsub {
                source_url: "https://example.com".to_string(),
            },
            &mut model,
        );

        assert_eq!(cmd, FeedListCmd::None);
        assert!(!model.marked_unsub.contains("https://example.com"));
    }

    #[test]
    fn test_save_with_marked_returns_save_cmd() {
        let mut model = FeedListModel::new(10);
        model.state = FeedListViewState::Edit;
        model.marked_unsub.insert("https://a.com".to_string());
        model.marked_unsub.insert("https://b.com".to_string());

        let cmd = FeedListUpdate::update(FeedListMsg::Save, &mut model);

        assert_eq!(model.state, FeedListViewState::View);
        assert!(model.marked_unsub.is_empty());
        match cmd {
            FeedListCmd::SaveUnsubscribes(urls) => {
                assert_eq!(urls.len(), 2);
                assert!(urls.contains("https://a.com"));
                assert!(urls.contains("https://b.com"));
            }
            other => panic!("expected SaveUnsubscribes, got {:?}", other),
        }
    }

    #[test]
    fn test_save_empty_returns_refetch() {
        let mut model = FeedListModel::new(10);
        model.state = FeedListViewState::Edit;

        let cmd = FeedListUpdate::update(FeedListMsg::Save, &mut model);

        assert_eq!(model.state, FeedListViewState::View);
        assert_eq!(cmd, FeedListCmd::RefetchSubscriptions);
    }

    #[test]
    fn test_pagination_first() {
        let mut model = FeedListModel::new(10);
        model.current_page = 5;

        let cmd =
            FeedListUpdate::update(FeedListMsg::Pagination(PaginationAction::First), &mut model);

        assert_eq!(cmd, FeedListCmd::RefetchSubscriptions);
        assert_eq!(model.current_page, 1);
    }

    #[test]
    fn test_pagination_prev() {
        let mut model = FeedListModel::new(10);
        model.current_page = 3;

        let cmd =
            FeedListUpdate::update(FeedListMsg::Pagination(PaginationAction::Prev), &mut model);

        assert_eq!(cmd, FeedListCmd::RefetchSubscriptions);
        assert_eq!(model.current_page, 2);
    }

    #[test]
    fn test_pagination_prev_does_not_go_below_one() {
        let mut model = FeedListModel::new(10);
        model.current_page = 1;

        let cmd =
            FeedListUpdate::update(FeedListMsg::Pagination(PaginationAction::Prev), &mut model);

        assert_eq!(cmd, FeedListCmd::RefetchSubscriptions);
        assert_eq!(model.current_page, 1);
    }

    #[test]
    fn test_model_new_defaults() {
        let model = FeedListModel::new(10);
        assert_eq!(model.state, FeedListViewState::View);
        assert!(model.marked_unsub.is_empty());
        assert_eq!(model.current_page, 1);
        assert_eq!(model.per_page, 10);
        assert!(!model.pagination_disabled);
    }
}
