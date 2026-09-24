use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use feed::Platforms;
use feed::entity::SubscriberEntity;
use feed::entity::SubscriberType;
use feed::host_client::HostCallError;
use feed::host_client::HostClient;
use feed::service::feed_subscription::FeedSubscriptionService;
use feed::subscriber::discord_dm::DiscordDmSubscriber;
use feed::subscriber::discord_guild::DiscordGuildSubscriber;
use pwr_plugin_protocol::FeedsSettings;
use serde_json::Value;
use serde_json::json;

#[allow(dead_code)]
mod common;

#[derive(Default)]
struct RecordingHost {
    calls: Mutex<Vec<(String, Value)>>,
    dm_error: Mutex<Option<HostCallError>>,
    send_error: Mutex<Option<HostCallError>>,
}

#[async_trait]
impl HostClient for RecordingHost {
    async fn call(&self, op: &str, args: Value) -> Result<Value, HostCallError> {
        self.calls.lock().unwrap().push((op.to_string(), args));
        if op == "host.open_dm"
            && let Some(error) = self.dm_error.lock().unwrap().clone()
        {
            return Err(error);
        }
        if op == "host.send_message"
            && let Some(error) = self.send_error.lock().unwrap().take()
        {
            return Err(error);
        }
        if op == "host.open_dm" {
            Ok(json!({ "channel_id": 900 }))
        } else {
            Ok(json!({ "message_id": 901 }))
        }
    }
}

fn subscriber(r#type: SubscriberType, target_id: &str) -> SubscriberEntity {
    SubscriberEntity {
        r#type,
        target_id: target_id.to_string(),
        ..SubscriberEntity::default()
    }
}

#[serial_test::serial]
#[tokio::test]
async fn dm_delivery_resolves_the_channel_once_and_reuses_it() {
    let repository = common::setup_db().await;
    let service = Arc::new(FeedSubscriptionService::new(
        &repository,
        Arc::new(Platforms::new()),
    ));
    let host = Arc::new(RecordingHost::default());
    let delivery = DiscordDmSubscriber::new(service, host.clone());
    let subscriber = subscriber(SubscriberType::Dm, "42");
    let message = json!({ "flags": 32768, "components": [] });

    delivery
        .handle_sub(&subscriber, message.clone())
        .await
        .expect("first DM");
    delivery
        .handle_sub(&subscriber, message)
        .await
        .expect("second DM");

    {
        let calls = host.calls.lock().unwrap();
        assert_eq!(
            calls.iter().map(|(op, _)| op.as_str()).collect::<Vec<_>>(),
            ["host.open_dm", "host.send_message", "host.send_message"]
        );
        assert_eq!(calls[1].1["channel_id"], json!(900));
        assert_eq!(calls[2].1["channel_id"], json!(900));
    }
    common::teardown_db(&repository).await;
}

#[serial_test::serial]
#[tokio::test]
async fn failed_dm_send_clears_the_cache_without_retrying() {
    let repository = common::setup_db().await;
    let service = Arc::new(FeedSubscriptionService::new(
        &repository,
        Arc::new(Platforms::new()),
    ));
    let host = Arc::new(RecordingHost::default());
    let delivery = DiscordDmSubscriber::new(service, host.clone());
    let subscriber = subscriber(SubscriberType::Dm, "42");
    let message = json!({ "flags": 32768, "components": [] });

    delivery
        .handle_sub(&subscriber, message.clone())
        .await
        .expect("cache the DM channel");
    *host.send_error.lock().unwrap() = Some(HostCallError::Wire {
        kind: "HostIoError".into(),
        msg: "stale channel".into(),
    });
    delivery
        .handle_sub(&subscriber, message.clone())
        .await
        .expect_err("a failed send is reported to the event loop");

    {
        let calls = host.calls.lock().unwrap();
        assert_eq!(
            calls.iter().map(|(op, _)| op.as_str()).collect::<Vec<_>>(),
            ["host.open_dm", "host.send_message", "host.send_message"]
        );
    }

    delivery
        .handle_sub(&subscriber, message)
        .await
        .expect("the next send reopens the DM channel");

    {
        let calls = host.calls.lock().unwrap();
        assert_eq!(
            calls.iter().map(|(op, _)| op.as_str()).collect::<Vec<_>>(),
            [
                "host.open_dm",
                "host.send_message",
                "host.send_message",
                "host.open_dm",
                "host.send_message"
            ]
        );
        assert_eq!(calls[3].1["user_id"], json!(42));
        assert_eq!(calls[4].1["channel_id"], json!(900));
    }
    common::teardown_db(&repository).await;
}

#[serial_test::serial]
#[tokio::test]
async fn failed_dm_resolution_is_not_cached() {
    let repository = common::setup_db().await;
    let service = Arc::new(FeedSubscriptionService::new(
        &repository,
        Arc::new(Platforms::new()),
    ));
    let host = Arc::new(RecordingHost::default());
    *host.dm_error.lock().unwrap() = Some(HostCallError::Wire {
        kind: "HostIoError".into(),
        msg: "unknown user".into(),
    });
    let delivery = DiscordDmSubscriber::new(service, host.clone());
    let subscriber = subscriber(SubscriberType::Dm, "42");

    delivery
        .handle_sub(&subscriber, json!({}))
        .await
        .expect_err("first resolution fails");
    *host.dm_error.lock().unwrap() = None;
    delivery
        .handle_sub(&subscriber, json!({}))
        .await
        .expect("retry resolves the DM");

    {
        let calls = host.calls.lock().unwrap();
        assert_eq!(
            calls.iter().map(|(op, _)| op.as_str()).collect::<Vec<_>>(),
            ["host.open_dm", "host.open_dm", "host.send_message"]
        );
    }
    common::teardown_db(&repository).await;
}

#[serial_test::serial]
#[tokio::test]
async fn guild_delivery_uses_the_configured_feed_channel() {
    let repository = common::setup_db().await;
    let service = Arc::new(FeedSubscriptionService::new(
        &repository,
        Arc::new(Platforms::new()),
    ));
    service
        .update_feed_settings(
            77,
            FeedsSettings {
                channel_id: Some("123".into()),
                ..FeedsSettings::default()
            },
        )
        .await
        .expect("save feed channel");
    let host = Arc::new(RecordingHost::default());
    let delivery = DiscordGuildSubscriber::new(service, host.clone());

    delivery
        .handle_sub(
            &subscriber(SubscriberType::Guild, "77"),
            json!({ "flags": 32768 }),
        )
        .await
        .expect("guild delivery");

    {
        let calls = host.calls.lock().unwrap();
        assert_eq!(calls[0].0, "host.send_message");
        assert_eq!(calls[0].1["channel_id"], json!(123));
    }
    common::teardown_db(&repository).await;
}
