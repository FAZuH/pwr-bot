//! Feed subscription types used by ViewEngine commands.
//!
//! Heavy logic lives in the feed plugin. These are just the types that
//! core commands still import during the transition.

use crate::entity::FeedEntity;
use crate::entity::FeedItemEntity;
use crate::entity::SubscriberEntity;
use crate::entity::SubscriberType;

pub enum SubscribeResult {
    Success { feed: FeedEntity },
    AlreadySubscribed { feed: FeedEntity },
}

pub enum UnsubscribeResult {
    Success { feed: FeedEntity },
    AlreadyUnsubscribed { feed: FeedEntity },
    NoneSubscribed { url: String },
}

#[derive(Debug, Clone)]
pub struct SubscriberTarget {
    pub subscriber_type: SubscriberType,
    pub target_id: String,
}

#[derive(Clone, Debug)]
pub struct Subscription {
    pub feed: FeedEntity,
    pub feed_latest: Option<FeedItemEntity>,
}
