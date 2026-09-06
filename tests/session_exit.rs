//! End-to-end test for the host-session exit seam (`#144`): the Router's
//! hub handoff adopts a live host message into the settings plugin's hub
//! view. The plugin is spawned over the real stdio wire with an in-memory
//! [`KvStore`]; the Discord edit is a mock ([`MockHostIo`]). Pure stdio —
//! no database, no Discord.
//!
//! Assertions mirror the handoff contract:
//! - `adopt_message_into_hub` invokes the plugin's hub view, morphs the
//!   message in place through the Discord-edit seam (the payload is
//!   Components V2 carrying the monolith hub), and only then registers the
//!   engine session on the message id;
//! - the adopted session behaves like any hub: an About click renders the
//!   plugin-side About panel and its Back restores the hub.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use poise::serenity_prelude as serenity;
use pwr_bot::bot::command::session_exit::adopt_message_into_hub;
use pwr_bot::plugin::HostConfig;
use pwr_bot::plugin::HostServices;
use pwr_bot::plugin::InteractionEngine;
use pwr_bot::plugin::KvError;
use pwr_bot::plugin::KvStore;
use pwr_bot::plugin::RunningPlugin;
use pwr_bot::plugin::StatsHandle;
use pwr_bot::plugin::host::MockHostIo;
use pwr_bot::plugin::validate_view_data;
use pwr_poise_components::IS_COMPONENTS_V2;
use serde_json::json;

mod probe;
use probe::probe_binary;

/// A small in-memory [`KvStore`]: the hub's invoke chain reads and
/// re-registers the model, so the store just has to answer get/set.
#[derive(Default)]
struct MemoryKv(Mutex<HashMap<(String, String), String>>);

#[async_trait]
impl KvStore for MemoryKv {
    async fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, KvError> {
        Ok(self
            .0
            .lock()
            .expect("kv lock")
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    async fn set(&self, namespace: &str, key: &str, value: &str) -> Result<(), KvError> {
        self.0
            .lock()
            .expect("kv lock")
            .insert((namespace.to_string(), key.to_string()), value.to_string());
        Ok(())
    }

    async fn delete(&self, namespace: &str, key: &str) -> Result<(), KvError> {
        self.0
            .lock()
            .expect("kv lock")
            .remove(&(namespace.to_string(), key.to_string()));
        Ok(())
    }
}

async fn spawn_settings() -> Arc<RunningPlugin> {
    Arc::new(
        RunningPlugin::spawn_with(
            probe_binary("settings"),
            Some(Arc::new(HostServices {
                io: None,
                config: Some(HostConfig {
                    db_url: "postgres://test".into(),
                    data_path: PathBuf::from("/tmp/pwr-bot-test"),
                    poll_interval: std::time::Duration::from_secs(30),
                }),
                kv: Some(Arc::new(MemoryKv::default())),
                engine: None,
                stats: Arc::new(StatsHandle::default()),
                feeds: None,
                voice: None,
            })),
            None,
            None,
        )
        .await
        .expect("spawn settings plugin"),
    )
}

/// The hub handoff adopts a live message into the settings plugin's hub:
/// one validated edit morphs it in place, the message id becomes a plugin
/// session, and that session answers About and Back like any hub.
#[tokio::test]
async fn the_adopted_message_answers_about_and_back() {
    let channel_id = 987_654_321_u64;
    let message_id = 123_456_789_u64;

    let mut mock = MockHostIo::new();
    mock.expect_edit_message()
        .with(
            mockall::predicate::eq(channel_id),
            mockall::predicate::eq(message_id),
            mockall::predicate::function(|data: &serde_json::Value| {
                data["flags"] == json!(IS_COMPONENTS_V2)
                    && data["components"][0]["components"][0]["content"] == json!("-# **Settings**")
                    // The hub view is a create envelope: the morph strips its
                    // create-only fields, which Discord rejects on edit.
                    && data.get("sticker_ids").is_none()
                    && data.get("tts").is_none()
                    && data.get("enforce_nonce").is_none()
            }),
        )
        .times(1)
        .returning(move |_, _, _| Ok(Some(json!({ "message_id": message_id }))));

    let engine = InteractionEngine::new();
    let plugin = spawn_settings().await;
    adopt_message_into_hub(
        &engine,
        plugin.clone(),
        &mock,
        serenity::ChannelId::new(channel_id),
        serenity::MessageId::new(message_id),
    )
    .await
    .expect("the hub handoff adopts the message");

    let message_id = serenity::MessageId::new(message_id);
    assert!(
        engine.has_session(message_id).await,
        "the adopted message carries a plugin session"
    );

    let about = engine
        .interact_validated(message_id, "settings:about", json!({}), |data| {
            validate_view_data(data).map_err(Into::into)
        })
        .await
        .expect("the About click routes to the adopted session");
    assert_eq!(
        about.data["components"][1]["components"][0]["custom_id"],
        json!("settings:about:back"),
        "the About panel carries the Back button"
    );

    let hub = engine
        .interact_validated(message_id, "settings:about:back", json!({}), |data| {
            validate_view_data(data).map_err(Into::into)
        })
        .await
        .expect("the Back click routes to the adopted session");
    assert_eq!(
        hub.data["components"][0]["components"][0]["content"],
        json!("-# **Settings**"),
        "Back restores the hub"
    );

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}
