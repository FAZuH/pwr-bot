//! Pure update logic for the feed subscription batch view.
//!
//! Holds the single source of truth for the batch results view
//! (`FeedBatchModel`), the exhaustive message vocabulary (`FeedBatchMsg`), and
//! an empty effect vocabulary (`FeedBatchEffect`). The per-URL subscription IO
//! happens in the shell command handler; this core only renders the accumulated
//! results and handles the "View Subscriptions" action that concludes the flow.

use crate::entity::SubscriberType;

/// The batch view phase.
///
/// Distinguishes the intermediate progress render (still processing, no
/// confirm action) from the final render (all results shown, with the confirm
/// action available).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedBatchPhase {
    /// Intermediate progress render — no confirm action.
    Confirm,
    /// Final render — shows the "View Subscriptions" action.
    Done,
}

/// The feed batch view model — the single source of truth for the view.
#[derive(Debug, Clone)]
pub struct FeedBatchModel {
    /// Per-URL result text, in the order they were processed.
    pub states: Vec<String>,
    /// Whether this render is the final (interactive) one.
    pub phase: FeedBatchPhase,
    /// The subscriber type, used to navigate to the right list on confirm.
    pub subscriber_type: SubscriberType,
}

impl FeedBatchModel {
    /// Constructs the model for one render of the batch results.
    pub fn new(
        states: Vec<String>,
        phase: FeedBatchPhase,
        subscriber_type: SubscriberType,
    ) -> Self {
        Self {
            states,
            phase,
            subscriber_type,
        }
    }
}

/// Messages that drive the feed batch view.
///
/// Exhaustive: every way the world can change the batch model is one variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedBatchMsg {
    /// Boot handshake — the host dispatches this on start.
    Start,
    /// The view loop timed out.
    Expired,
    /// The "View Subscriptions" action was pressed.
    ViewSubscriptions { subscriber_type: SubscriberType },
}

/// Effects the feed batch view can request.
///
/// Empty: the batch view performs no side effects — the subscription IO happens
/// in the shell command handler, and navigation is a host concern handled via
/// [`FeedBatchMsg`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedBatchEffect {}

/// The pure update function.
///
/// Each render is a fresh model built by the shell, so every message is a
/// no-op that returns no effects.
pub fn update(_msg: FeedBatchMsg, _model: &mut FeedBatchModel) -> Vec<FeedBatchEffect> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> FeedBatchModel {
        FeedBatchModel::new(
            vec!["Subscribed to https://a.com".to_string()],
            FeedBatchPhase::Confirm,
            SubscriberType::Dm,
        )
    }

    #[test]
    fn start_is_a_noop() {
        let mut m = model();
        let effects = update(FeedBatchMsg::Start, &mut m);
        assert!(effects.is_empty());
    }

    #[test]
    fn expired_is_a_noop() {
        let mut m = model();
        let effects = update(FeedBatchMsg::Expired, &mut m);
        assert!(effects.is_empty());
    }

    #[test]
    fn view_subscriptions_is_a_noop() {
        let mut m = model();
        let effects = update(
            FeedBatchMsg::ViewSubscriptions {
                subscriber_type: SubscriberType::Dm,
            },
            &mut m,
        );
        assert!(effects.is_empty());
    }
}
