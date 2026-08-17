//! End-to-end tests for the settings core plugin (the first real core plugin,
//! #115): the plugin is spawned over the real stdio wire with a stateful
//! in-memory [`KvStore`] (hand-rolled — the mockall `MockKvStore` is
//! per-call stateless, but the settings flow needs persistence across two
//! spawned instances). Pure stdio — no database, no Discord.
//!
//! Assertions mirror the plugin's documented contract:
//! - an `invoke` of `settings` answers the full envelope
//!   `{"data", "ephemeral", "view"}` with one action row of three toggles and
//!   the loaded (or default) model in `view`;
//! - the first invoke loads the model from `host.kv.get` (namespace
//!   `settings`) and re-registers it via `host.kv.set` before rendering;
//! - `view.interact` toggles the feature and persists the model via
//!   `host.kv.set` before answering;
//! - a second spawn sharing the same store renders the persisted model;
//! - `bye` exits cleanly with status 0.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use pwr_bot::plugin::HostConfig;
use pwr_bot::plugin::HostServices;
use pwr_bot::plugin::KvError;
use pwr_bot::plugin::KvStore;
use pwr_bot::plugin::RunningPlugin;
use pwr_plugin_protocol::Msg;
use serde_json::Value;
use serde_json::json;

/// The KV namespace and model key the settings plugin persists under.
const KV_NAMESPACE: &str = "settings";
const KV_MODEL_KEY: &str = "model";

/// Locates the `pwr-plugin-settings` binary. `CARGO_BIN_EXE_...` is only set
/// for the crate's own tests; from the host crate the workspace build places
/// the binary under `target/{profile}`. Probe `debug` and `release` like
/// `plugin_host_ops::fixture_path`.
fn settings_path() -> PathBuf {
    let target = match option_env!("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"),
    };
    for profile in ["debug", "release"] {
        let candidate = target.join(profile).join("pwr-plugin-settings");
        if candidate.exists() {
            return candidate;
        }
    }
    panic!(concat!(
        "pwr-plugin-settings not built; run `cargo build -p pwr-plugin-settings` ",
        "(or `cargo build --workspace`) first"
    ));
}

/// A stateful in-memory [`KvStore`]: a `(namespace, key)` → value map shared
/// across every handle that holds it, so a second spawned instance observes
/// what the first persisted.
#[derive(Default)]
struct SharedKv {
    inner: Mutex<HashMap<(String, String), String>>,
}

impl SharedKv {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The persisted value for the settings model, if any.
    fn model_value(&self) -> Option<String> {
        self.inner
            .lock()
            .expect("kv lock")
            .get(&(KV_NAMESPACE.to_string(), KV_MODEL_KEY.to_string()))
            .cloned()
    }
}

#[async_trait]
impl KvStore for SharedKv {
    async fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, KvError> {
        Ok(self
            .inner
            .lock()
            .expect("kv lock")
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    async fn set(&self, namespace: &str, key: &str, value: &str) -> Result<(), KvError> {
        self.inner
            .lock()
            .expect("kv lock")
            .insert((namespace.to_string(), key.to_string()), value.to_string());
        Ok(())
    }

    async fn delete(&self, namespace: &str, key: &str) -> Result<(), KvError> {
        self.inner
            .lock()
            .expect("kv lock")
            .remove(&(namespace.to_string(), key.to_string()));
        Ok(())
    }
}

fn host_services(kv: Option<Arc<dyn KvStore>>) -> Arc<HostServices> {
    Arc::new(HostServices {
        io: None,
        config: Some(HostConfig {
            db_url: "postgres://test".into(),
            data_path: PathBuf::from("/tmp/pwr-bot-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv,
        engine: None,
    })
}

/// Spawns the settings plugin with the given KV store (or none).
async fn spawn_settings(kv: Option<Arc<dyn KvStore>>) -> RunningPlugin {
    RunningPlugin::spawn_with(settings_path(), Some(host_services(kv)), None, None)
        .await
        .expect("spawn settings plugin")
}

/// Asserts a resp is the settings envelope and returns its `view` object.
/// `expected_id` is the host's call id this resp answers: each sequential
/// `plugin.call` on the same instance bumps the id (0, 1, 2, ...), and the
/// plugin echoes it back end to end.
fn assert_envelope(resp: &Msg, expected_id: u64) -> Value {
    match resp {
        Msg::Resp {
            id,
            ok: true,
            data: Some(data),
            error: None,
        } => {
            assert_eq!(*id, expected_id, "resp answers its own invoke id");
            assert_eq!(data["ephemeral"], json!(false));
            assert_eq!(data["data"]["content"], json!("-# **Settings**"));
            data["view"].clone()
        }
        other => panic!("expected ok settings envelope, got {other:?}"),
    }
}

/// Asserts the envelope's action row carries the three feature toggles with
/// the given enabled state in their labels and styles.
fn assert_toggles(view: &Value, feeds: bool, voice: bool, welcome: bool) {
    let model = view.as_object().expect("view is an object");
    assert_eq!(model["feeds"], json!(feeds));
    assert_eq!(model["voice"], json!(voice));
    assert_eq!(model["welcome"], json!(welcome));
}

/// Asserts the envelope's message data has one action row of three buttons
/// with the expected custom ids.
fn assert_buttons(data: &Value, enabled: &[bool]) {
    let rows = data["data"]["components"].as_array().expect("components");
    assert_eq!(rows.len(), 1, "one action row");
    let row = &rows[0];
    assert_eq!(row["type"], json!(1));
    let buttons = row["components"].as_array().expect("row components");
    assert_eq!(buttons.len(), 3, "three toggle buttons");

    let custom_ids = ["settings:feeds", "settings:voice", "settings:welcome"];
    for (button, (custom_id, is_enabled)) in buttons.iter().zip(custom_ids.iter().zip(enabled)) {
        assert_eq!(button["type"], json!(2));
        assert_eq!(button["custom_id"], json!(custom_id));
        // style 3 (success) when enabled, 2 (secondary) when disabled
        assert_eq!(button["style"], json!(if *is_enabled { 3 } else { 2 }));
    }
}

/// An `invoke` of `settings` answers the full envelope with the default model
/// (every feature disabled) after loading from and re-registering in KV.
#[tokio::test]
async fn invoke_answers_the_default_envelope_and_registers_in_kv() {
    let kv = SharedKv::new();
    let plugin = spawn_settings(Some(kv.clone())).await;

    let resp = plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("invoke answered");

    let view = assert_envelope(&resp, 0);
    assert_toggles(&view, false, false, false);
    // The first invoke loaded from KV (empty → default) and re-registered the
    // model before rendering (AC3 registration contract).
    assert_eq!(
        kv.model_value(),
        Some(
            json!({
                "feeds": false,
                "voice": false,
                "welcome": false
            })
            .to_string()
        ),
        "model registered in KV"
    );

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// An `invoke` while the model is already loaded answers immediately with the
/// current model — no further KV traffic is needed.
#[tokio::test]
async fn second_invoke_answers_immediately_with_the_loaded_model() {
    let kv = SharedKv::new();
    let plugin = spawn_settings(Some(kv.clone())).await;

    plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("first invoke answered");

    let resp = plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("second invoke answered");

    let view = assert_envelope(&resp, 1);
    assert_toggles(&view, false, false, false);

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// A `view.interact` click toggles the feature, persists the model through
/// `host.kv.set` (namespace `settings`), and answers the fresh envelope.
#[tokio::test]
async fn interact_toggles_a_feature_and_persists_it() {
    let kv = SharedKv::new();
    let plugin = spawn_settings(Some(kv.clone())).await;

    plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("invoke answered");

    let resp = plugin
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({ "custom_id": "settings:feeds" })),
        )
        .await
        .expect("interact answered");

    let view = assert_envelope(&resp, 1);
    assert_toggles(&view, true, false, false);
    assert_eq!(
        kv.model_value(),
        Some(
            json!({
                "feeds": true,
                "voice": false,
                "welcome": false
            })
            .to_string()
        ),
        "toggled model persisted in KV"
    );

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// A second spawned instance sharing the same KV store renders the persisted
/// model: state survives a restart (AC6 persistence proof).
#[tokio::test]
async fn a_restarted_plugin_renders_the_persisted_model() {
    let kv = SharedKv::new();

    // First instance: toggle feeds on, then stop.
    let first = spawn_settings(Some(kv.clone())).await;
    first
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("first invoke answered");
    first
        .call(
            "view.interact",
            Some("settings"),
            Some(json!({ "custom_id": "settings:feeds" })),
        )
        .await
        .expect("first toggle answered");
    let status = first.stop().await.expect("first stop");
    assert_eq!(status.code(), Some(0), "first instance exits 0: {status}");

    // Second instance, same store: the first invoke must load feeds=true.
    let second = spawn_settings(Some(kv.clone())).await;
    let resp = second
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("second invoke answered");
    let view = assert_envelope(&resp, 0);
    assert_toggles(&view, true, false, false);
    let status = second.stop().await.expect("second stop");
    assert_eq!(status.code(), Some(0), "second instance exits 0: {status}");
}

/// An envelope's action row carries the three toggles with labels reflecting
/// the current state.
#[tokio::test]
async fn envelope_carries_three_toggle_buttons() {
    let plugin = spawn_settings(None).await;

    let resp = plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("invoke answered");

    let Msg::Resp {
        data: Some(data), ..
    } = &resp
    else {
        panic!("expected ok envelope, got {resp:?}");
    };
    assert_buttons(data, &[false, false, false]);

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

/// Without a KV store the plugin still answers: the load failure falls back to
/// the default model and the render is not blocked (the failed save is
/// logged, not fatal).
#[tokio::test]
async fn invoke_answers_with_defaults_when_kv_is_unavailable() {
    let plugin = spawn_settings(None).await;

    let resp = plugin
        .call("invoke", Some("settings"), Some(json!({})))
        .await
        .expect("invoke answered");

    let view = assert_envelope(&resp, 0);
    assert_toggles(&view, false, false, false);

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}
