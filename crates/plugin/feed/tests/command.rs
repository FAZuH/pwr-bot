use std::sync::Arc;

use feed::Platforms;
use feed::command::FeedListSession;
use feed::command::interact_list;
use feed::entity::SubscriberEntity;
use feed::entity::SubscriberType;
use feed::service::feed_subscription::FeedSubscriptionService;
use feed::update::feed_list::FeedListModel;
use serde_json::json;

#[allow(dead_code)]
mod common;

#[serial_test::serial]
#[tokio::test]
async fn a_failed_list_refetch_returns_an_error_instead_of_an_empty_page() {
    let repository = common::setup_db().await;
    let service = FeedSubscriptionService::new(&repository, Arc::new(Platforms::new()));
    let session = FeedListSession {
        model: FeedListModel::new(Vec::new(), 10),
        subscriber: SubscriberEntity {
            id: 1,
            r#type: SubscriberType::Dm,
            target_id: "42".into(),
        },
    };
    let args = json!({
        "custom_id": "feed-list:next",
        "view": serde_json::to_value(session).expect("serialize list session"),
    });
    repository.pool().close();

    let result = interact_list(&service, &args).await;

    assert!(result.is_err(), "a failed refetch is not an empty success");
}
