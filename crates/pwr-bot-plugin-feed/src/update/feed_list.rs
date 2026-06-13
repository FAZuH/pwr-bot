use std::collections::HashSet;

use crate::update::PaginationAction;
use crate::update::PaginationModel;

/// View state for the feed list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedListViewState {
    View,
    Edit,
}

/// Messages that can mutate the feed list model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedListMsg {
    Edit,
    View,
    ToggleUnsub(String),
    Save,
    Pagination(PaginationAction),
}

/// Commands returned by the update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedListCmd {
    None,
    SaveUnsubscribes(HashSet<String>),
    RefetchSubscriptions,
}

/// The feed list model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedListModel {
    pub state: FeedListViewState,
    pub marked_unsub: HashSet<String>,
    pub pagination: PaginationModel,
}

impl FeedListModel {
    pub fn new(total_pages: u32) -> Self {
        Self {
            state: FeedListViewState::View,
            marked_unsub: HashSet::new(),
            pagination: PaginationModel::new(total_pages, 1),
        }
    }
}

/// Pure update function for the feed list.
pub fn feed_list_update(
    msg: FeedListMsg,
    model: &mut FeedListModel,
) -> FeedListCmd {
    use FeedListCmd::*;
    use FeedListMsg::*;

    match msg {
        Edit => {
            model.state = FeedListViewState::Edit;
            None
        }
        View => {
            model.state = FeedListViewState::View;
            None
        }
        ToggleUnsub(url) => {
            if model.marked_unsub.contains(&url) {
                model.marked_unsub.remove(&url);
            } else {
                model.marked_unsub.insert(url);
            }
            None
        }
        Save => {
            let urls = std::mem::take(&mut model.marked_unsub);
            if urls.is_empty() {
                None
            } else {
                SaveUnsubscribes(urls)
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

    fn default_model() -> FeedListModel {
        FeedListModel::new(5)
    }

    #[test]
    fn model_new_defaults() {
        let m = default_model();
        assert_eq!(m.state, FeedListViewState::View);
        assert!(m.marked_unsub.is_empty());
        assert_eq!(m.pagination.current_page, 1);
    }

    #[test]
    fn edit_sets_state() {
        let mut m = default_model();
        let cmd = feed_list_update(FeedListMsg::Edit, &mut m);
        assert_eq!(cmd, FeedListCmd::None);
        assert_eq!(m.state, FeedListViewState::Edit);
    }

    #[test]
    fn view_sets_state() {
        let mut m = default_model();
        feed_list_update(FeedListMsg::Edit, &mut m);
        feed_list_update(FeedListMsg::View, &mut m);
        assert_eq!(m.state, FeedListViewState::View);
    }

    #[test]
    fn toggle_unsub_adds() {
        let mut m = default_model();
        feed_list_update(FeedListMsg::ToggleUnsub("url1".into()), &mut m);
        assert!(m.marked_unsub.contains("url1"));
    }

    #[test]
    fn toggle_unsub_removes() {
        let mut m = default_model();
        feed_list_update(FeedListMsg::ToggleUnsub("url1".into()), &mut m);
        feed_list_update(FeedListMsg::ToggleUnsub("url1".into()), &mut m);
        assert!(!m.marked_unsub.contains("url1"));
    }

    #[test]
    fn save_empty_returns_refetch() {
        let mut m = default_model();
        let cmd = feed_list_update(FeedListMsg::Save, &mut m);
        assert_eq!(cmd, FeedListCmd::None);
    }

    #[test]
    fn save_with_marked_returns_save_cmd() {
        let mut m = default_model();
        feed_list_update(FeedListMsg::ToggleUnsub("url1".into()), &mut m);
        let cmd = feed_list_update(FeedListMsg::Save, &mut m);
        assert_eq!(
            cmd,
            FeedListCmd::SaveUnsubscribes(HashSet::from(["url1".into()]))
        );
        assert!(m.marked_unsub.is_empty());
    }

    #[test]
    fn pagination_first() {
        let mut m = default_model();
        m.pagination = PaginationModel::new(5, 3);
        feed_list_update(FeedListMsg::Pagination(PaginationAction::First), &mut m);
        assert_eq!(m.pagination.current_page, 1);
    }

    #[test]
    fn pagination_prev() {
        let mut m = default_model();
        m.pagination = PaginationModel::new(5, 3);
        feed_list_update(FeedListMsg::Pagination(PaginationAction::Prev), &mut m);
        assert_eq!(m.pagination.current_page, 2);
    }

    #[test]
    fn pagination_prev_does_not_go_below_one() {
        let mut m = default_model();
        m.pagination = PaginationModel::new(5, 1);
        feed_list_update(FeedListMsg::Pagination(PaginationAction::Prev), &mut m);
        assert_eq!(m.pagination.current_page, 1);
    }
}
