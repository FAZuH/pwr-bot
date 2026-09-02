//! Pure update logic for the feed subscription list.
//!
//! Holds the single source of truth for the feed list view
//! (`FeedListModel`), which absorbs the subscription list, the view/edit mode,
//! the mark-for-unsubscribe set, and the pagination state. Also holds the
//! exhaustive message vocabulary (`FeedListMsg`) and the data-only effect
//! vocabulary (`FeedListEffect`).
//!
//! Initial subscriptions arrive at model construction (shell-legal data-in via
//! the feature `Config`, per the boot-load directive); in-session refetches
//! (pagination, post-save reload) are [`FeedListEffect`]s executed by the
//! adapter, whose results flow back as messages ([`FeedListMsg::Saved`],
//! [`FeedListMsg::SubscriptionsLoaded`]).

use std::collections::HashSet;

use crate::service::feed_subscription::Subscription;
use crate::update::pagination::PaginationAction;

/// View state for the feed list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedListViewState {
    View,
    Edit,
}

/// The feed list view model — the single source of truth for the view.
#[derive(Debug, Clone)]
pub struct FeedListModel {
    /// The subscriptions currently displayed (the active page).
    pub(crate) subscriptions: Vec<Subscription>,
    pub(crate) state: FeedListViewState,
    /// The subscriptions marked for removal while in edit mode.
    pub(crate) marked_unsub: HashSet<String>,
    pub(crate) current_page: u32,
    pub(crate) per_page: u32,
    pub(crate) pagination_disabled: bool,
}

impl FeedListModel {
    /// Constructs the model from the first page of subscriptions.
    pub fn new(subscriptions: Vec<Subscription>, per_page: u32) -> Self {
        Self {
            subscriptions,
            state: FeedListViewState::View,
            marked_unsub: HashSet::new(),
            current_page: 1,
            per_page: per_page.max(1),
            pagination_disabled: false,
        }
    }

    /// The subscriptions currently displayed.
    pub fn subscriptions(&self) -> &[Subscription] {
        &self.subscriptions
    }

    /// The current view/edit mode.
    pub fn state(&self) -> FeedListViewState {
        self.state
    }

    /// The set of subscriptions marked for removal.
    pub fn marked_unsub(&self) -> &HashSet<String> {
        &self.marked_unsub
    }

    /// The current page number.
    pub fn current_page(&self) -> u32 {
        self.current_page
    }

    /// The page size.
    pub fn per_page(&self) -> u32 {
        self.per_page
    }

    /// Whether pagination is disabled (e.g. after a timeout).
    pub fn pagination_disabled(&self) -> bool {
        self.pagination_disabled
    }
}

/// Messages that drive the feed list view.
///
/// Exhaustive: every way the world can change the feed list model is one
/// variant.
#[derive(Debug, Clone)]
pub enum FeedListMsg {
    /// Boot handshake — the host dispatches this on start.
    Start,
    /// The view loop timed out.
    Expired,
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
    /// The adapter confirmed a save and is ready to be reloaded.
    Saved,
    /// The adapter loaded a fresh page of subscriptions.
    SubscriptionsLoaded(Vec<Subscription>),
}

/// Effects the feed list view can request.
///
/// Data-only. In-session async data fetching and unsubscribing are both driven
/// through [`FeedListEffect`], executed by the shell adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedListEffect {
    /// Refetch the subscriptions for the given page.
    QuerySubscriptions { page: u32, per_page: u32 },
    /// Perform the actual unsubscriptions.
    SaveUnsubscribes(HashSet<String>),
}

/// The pure update function — the only writer of the model.
///
/// `Start` is a no-op (initial data arrives via the config). Pagination and
/// post-save reloads return a [`FeedListEffect`] so the adapter refetches; the
/// returned [`FeedListMsg::SubscriptionsLoaded`] replaces the page in place.
pub fn update(msg: FeedListMsg, model: &mut FeedListModel) -> Vec<FeedListEffect> {
    match msg {
        FeedListMsg::Start => Vec::new(),
        FeedListMsg::Expired => {
            model.pagination_disabled = true;
            Vec::new()
        }
        FeedListMsg::Edit => {
            model.state = FeedListViewState::Edit;
            Vec::new()
        }
        FeedListMsg::View => {
            model.state = FeedListViewState::View;
            Vec::new()
        }
        FeedListMsg::ToggleUnsub { source_url } => {
            if model.marked_unsub.contains(&source_url) {
                model.marked_unsub.remove(&source_url);
            } else {
                model.marked_unsub.insert(source_url);
            }
            Vec::new()
        }
        FeedListMsg::Save => {
            let to_remove: HashSet<String> = std::mem::take(&mut model.marked_unsub);
            model.state = FeedListViewState::View;
            if to_remove.is_empty() {
                vec![FeedListEffect::QuerySubscriptions {
                    page: model.current_page,
                    per_page: model.per_page,
                }]
            } else {
                vec![FeedListEffect::SaveUnsubscribes(to_remove)]
            }
        }
        FeedListMsg::Saved => vec![FeedListEffect::QuerySubscriptions {
            page: model.current_page,
            per_page: model.per_page,
        }],
        FeedListMsg::SubscriptionsLoaded(subscriptions) => {
            model.subscriptions = subscriptions;
            Vec::new()
        }
        FeedListMsg::Pagination(action) => {
            match action {
                PaginationAction::First => model.current_page = 1,
                PaginationAction::Prev => {
                    model.current_page = model.current_page.saturating_sub(1).max(1);
                }
                PaginationAction::Next | PaginationAction::Last | PaginationAction::Page => {}
            }
            vec![FeedListEffect::QuerySubscriptions {
                page: model.current_page,
                per_page: model.per_page,
            }]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::FeedEntity;
    use crate::entity::FeedItemEntity;

    fn make_sub(name: &str, url: &str) -> Subscription {
        Subscription {
            feed: FeedEntity {
                id: 1,
                name: name.to_string(),
                description: "desc".to_string(),
                platform_id: "anilist".to_string(),
                source_id: "src".to_string(),
                items_id: "items".to_string(),
                source_url: url.to_string(),
                cover_url: format!("https://cover.example.com/{name}.png"),
                tags: String::new(),
            },
            feed_latest: Some(FeedItemEntity {
                id: 1,
                feed_id: 1,
                description: "v1".to_string(),
                published: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            }),
        }
    }

    fn model() -> FeedListModel {
        FeedListModel::new(
            vec![
                make_sub("Alpha", "https://alpha.example.com/feed"),
                make_sub("Beta", "https://beta.example.com/feed"),
            ],
            10,
        )
    }

    #[test]
    fn start_is_a_noop() {
        let mut m = model();
        let effects = update(FeedListMsg::Start, &mut m);
        assert!(effects.is_empty());
        assert_eq!(m.state(), FeedListViewState::View);
        assert_eq!(m.current_page(), 1);
    }

    #[test]
    fn expired_disables_pagination() {
        let mut m = model();
        let effects = update(FeedListMsg::Expired, &mut m);
        assert!(effects.is_empty());
        assert!(m.pagination_disabled());
    }

    #[test]
    fn edit_sets_state() {
        let mut m = model();
        let effects = update(FeedListMsg::Edit, &mut m);
        assert!(effects.is_empty());
        assert_eq!(m.state(), FeedListViewState::Edit);
    }

    #[test]
    fn view_sets_state() {
        let mut m = model();
        update(FeedListMsg::Edit, &mut m);
        let effects = update(FeedListMsg::View, &mut m);
        assert!(effects.is_empty());
        assert_eq!(m.state(), FeedListViewState::View);
    }

    #[test]
    fn toggle_unsub_adds() {
        let mut m = model();
        let effects = update(
            FeedListMsg::ToggleUnsub {
                source_url: "https://alpha.example.com/feed".to_string(),
            },
            &mut m,
        );
        assert!(effects.is_empty());
        assert!(m.marked_unsub().contains("https://alpha.example.com/feed"));
    }

    #[test]
    fn toggle_unsub_removes() {
        let mut m = model();
        m.marked_unsub
            .insert("https://alpha.example.com/feed".to_string());
        update(
            FeedListMsg::ToggleUnsub {
                source_url: "https://alpha.example.com/feed".to_string(),
            },
            &mut m,
        );
        assert!(!m.marked_unsub().contains("https://alpha.example.com/feed"));
    }

    #[test]
    fn save_with_marked_returns_save_effect() {
        let mut m = model();
        update(FeedListMsg::Edit, &mut m);
        m.marked_unsub
            .insert("https://alpha.example.com/feed".to_string());
        m.marked_unsub
            .insert("https://beta.example.com/feed".to_string());

        let effects = update(FeedListMsg::Save, &mut m);
        assert_eq!(m.state(), FeedListViewState::View);
        assert!(m.marked_unsub().is_empty());
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            FeedListEffect::SaveUnsubscribes(urls) => {
                assert_eq!(urls.len(), 2);
                assert!(urls.contains("https://alpha.example.com/feed"));
                assert!(urls.contains("https://beta.example.com/feed"));
            }
            other => panic!("expected SaveUnsubscribes, got {other:?}"),
        }
    }

    #[test]
    fn save_empty_returns_query() {
        let mut m = model();
        update(FeedListMsg::Edit, &mut m);

        let effects = update(FeedListMsg::Save, &mut m);
        assert_eq!(m.state(), FeedListViewState::View);
        assert_eq!(
            effects,
            vec![FeedListEffect::QuerySubscriptions {
                page: 1,
                per_page: 10,
            }]
        );
    }

    #[test]
    fn saved_returns_query() {
        let mut m = model();
        m.current_page = 2;
        let effects = update(FeedListMsg::Saved, &mut m);
        assert_eq!(
            effects,
            vec![FeedListEffect::QuerySubscriptions {
                page: 2,
                per_page: 10,
            }]
        );
    }

    #[test]
    fn subscriptions_loaded_replaces_page() {
        let mut m = model();
        let new_subs = vec![make_sub("Gamma", "https://gamma.example.com/feed")];
        let effects = update(FeedListMsg::SubscriptionsLoaded(new_subs.clone()), &mut m);
        assert!(effects.is_empty());
        assert_eq!(m.subscriptions().len(), 1);
        assert_eq!(
            m.subscriptions()[0].feed.source_url,
            "https://gamma.example.com/feed"
        );
    }

    #[test]
    fn pagination_first() {
        let mut m = model();
        m.current_page = 5;
        let effects = update(FeedListMsg::Pagination(PaginationAction::First), &mut m);
        assert_eq!(effects.len(), 1);
        assert_eq!(m.current_page(), 1);
    }

    #[test]
    fn pagination_prev() {
        let mut m = model();
        m.current_page = 3;
        let effects = update(FeedListMsg::Pagination(PaginationAction::Prev), &mut m);
        assert_eq!(effects.len(), 1);
        assert_eq!(m.current_page(), 2);
    }

    #[test]
    fn pagination_prev_does_not_go_below_one() {
        let mut m = model();
        m.current_page = 1;
        let effects = update(FeedListMsg::Pagination(PaginationAction::Prev), &mut m);
        assert_eq!(effects.len(), 1);
        assert_eq!(m.current_page(), 1);
    }

    #[test]
    fn pagination_next_and_last_are_noops() {
        let mut m = model();
        m.current_page = 2;
        update(FeedListMsg::Pagination(PaginationAction::Next), &mut m);
        assert_eq!(m.current_page(), 2);
        update(FeedListMsg::Pagination(PaginationAction::Last), &mut m);
        assert_eq!(m.current_page(), 2);
        update(FeedListMsg::Pagination(PaginationAction::Page), &mut m);
        assert_eq!(m.current_page(), 2);
    }

    #[test]
    fn model_new_defaults() {
        let m = FeedListModel::new(vec![], 10);
        assert_eq!(m.state(), FeedListViewState::View);
        assert!(m.marked_unsub().is_empty());
        assert_eq!(m.current_page(), 1);
        assert_eq!(m.per_page(), 10);
        assert!(!m.pagination_disabled());
        assert!(m.subscriptions().is_empty());
    }
}
