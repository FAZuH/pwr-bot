//! Integration tests for plugin→host `host.*` capability ops: the fixture
//! issues a `host.send_message` call through the real stdio wire, the host
//! serves it via the mocked [`HostIo`] seam (no live Discord), and the
//! fixture's resp echoes the mock's data back to the test. Pure stdio — no
//! database.
//!
//! `defer`/`edit_message`/`kv.get`/`kv.set`/`kv.delete` have fixture invoke
//! paths too; the kv ops are served via the mocked [`KvStore`] seam.
//! `acknowledge` is exercised against the mock in `src/plugin/host.rs` unit
//! tests.

use std::path::PathBuf;
use std::sync::Arc;

use pwr_bot::plugin::HostConfig;
use pwr_bot::plugin::HostIo;
use pwr_bot::plugin::HostServices;
use pwr_bot::plugin::KvStore;
use pwr_bot::plugin::RunningPlugin;
use pwr_bot::plugin::host::MockHostIo;
use pwr_bot::plugin::host::MockKvStore;
use pwr_plugin_protocol::Msg;
use serde_json::json;

/// Locates the `hello_plugin` fixture binary. `CARGO_BIN_EXE_hello_plugin`
/// is set by cargo for the protocol crate's own tests; for host-crate tests
/// the workspace build places the binary under `target/{profile}`. The test
/// binary does not expose the active profile, so probe `debug` and `release`
/// instead of guessing: CI (`cargo build --all-targets`) and local
/// `cargo build --workspace` both land the fixture in one of the two.
/// A missing binary panics with a build hint rather than a confusing spawn
/// error.
fn fixture_path() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_hello_plugin") {
        return PathBuf::from(path);
    }
    let target = match option_env!("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"),
    };
    for profile in ["debug", "release"] {
        let candidate = target.join(profile).join("hello_plugin");
        if candidate.exists() {
            return candidate;
        }
    }
    panic!(concat!(
        "test-plugin fixture not built; run `cargo build -p pwr-plugin-protocol ",
        "--bin hello_plugin` (or `cargo build --workspace`) first"
    ));
}

fn host_services(io: Arc<dyn HostIo>, kv: Option<Arc<dyn KvStore>>) -> Arc<HostServices> {
    Arc::new(HostServices {
        io: Some(io),
        config: Some(HostConfig {
            db_url: "postgres://test".into(),
            data_path: PathBuf::from("/tmp/pwr-bot-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv,
    })
}

/// The fixture's `host.say` invoke issues a plugin→host `host.send_message`
/// call; the host serves it through the mock seam and the fixture echoes the
/// mock's data back. Asserts the mock received the exact call and the resp
/// carried the fixture's correlation id.
#[tokio::test]
async fn host_say_serves_send_message_through_the_seam() {
    let mut mock = MockHostIo::new();
    mock.expect_send_message()
        .with(
            mockall::predicate::eq(987_654_321_u64),
            mockall::predicate::eq("hello from the fixture"),
            mockall::predicate::eq(None::<serde_json::Value>),
        )
        .times(1)
        .returning(|_, _, _| Ok(Some(json!({ "message_id": 123_456_789 }))));

    let plugin = RunningPlugin::spawn_with(
        fixture_path(),
        Some(host_services(Arc::new(mock), None)),
        None,
    )
    .await
    .expect("spawn fixture");
    let resp = plugin
        .call(
            "invoke",
            Some("host.say"),
            Some(json!({
                "channel_id": 987_654_321,
                "content": "hello from the fixture",
            })),
        )
        .await
        .expect("host.say invoke answered");
    plugin.stop().await.expect("stop fixture");

    match resp {
        Msg::Resp {
            id,
            ok: true,
            data: Some(data),
            error: None,
        } => {
            assert_eq!(data, json!({ "message_id": 123_456_789 }));
            // resp id 0 = the host's original invoke id, proving the fixture's
            // pending-map forwarding held end to end.
            assert_eq!(id, 0);
        }
        other => panic!("expected ok resp echoing the mock data, got {other:?}"),
    }
}

/// The `host.defer` invoke issues a plugin→host `host.defer` call; the host
/// serves it through the mock seam and the fixture forwards the resp (no
/// data) to the original invoke.
#[tokio::test]
async fn host_defer_invoke_serves_defer_through_the_seam() {
    let mut mock = MockHostIo::new();
    mock.expect_defer()
        .with(
            mockall::predicate::eq(555_444_333_u64),
            mockall::predicate::eq("defer-token"),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    let plugin = RunningPlugin::spawn_with(
        fixture_path(),
        Some(host_services(Arc::new(mock), None)),
        None,
    )
    .await
    .expect("spawn fixture");
    let resp = plugin
        .call(
            "invoke",
            Some("host.defer"),
            Some(json!({
                "interaction_id": 555_444_333,
                "token": "defer-token",
            })),
        )
        .await
        .expect("host.defer invoke answered");
    plugin.stop().await.expect("stop fixture");

    match resp {
        Msg::Resp {
            id,
            ok: true,
            data: None,
            error: None,
        } => assert_eq!(id, 0),
        other => panic!("expected ok resp with no data, got {other:?}"),
    }
}

/// The `host.edit` invoke issues a plugin→host `host.edit_message` call with
/// the payload; the host serves it through the mock seam and the fixture
/// echoes the mock's data back to the original invoke.
#[tokio::test]
async fn host_edit_invoke_serves_edit_message_through_the_seam() {
    let mut mock = MockHostIo::new();
    mock.expect_edit_message()
        .with(
            mockall::predicate::eq(777_888_999_u64),
            mockall::predicate::eq(111_222_333_u64),
            mockall::predicate::eq(json!({ "content": "edited" })),
        )
        .times(1)
        .returning(|_, _, _| Ok(Some(json!({ "message_id": 111_222_333 }))));

    let plugin = RunningPlugin::spawn_with(
        fixture_path(),
        Some(host_services(Arc::new(mock), None)),
        None,
    )
    .await
    .expect("spawn fixture");
    let resp = plugin
        .call(
            "invoke",
            Some("host.edit"),
            Some(json!({
                "channel_id": 777_888_999,
                "message_id": 111_222_333,
                "data": { "content": "edited" },
            })),
        )
        .await
        .expect("host.edit invoke answered");
    plugin.stop().await.expect("stop fixture");

    match resp {
        Msg::Resp {
            id,
            ok: true,
            data: Some(data),
            error: None,
        } => {
            assert_eq!(data, json!({ "message_id": 111_222_333 }));
            assert_eq!(id, 0);
        }
        other => panic!("expected ok resp echoing the mock data, got {other:?}"),
    }
}

/// Without services wired, a `host.*` call answers `HostUnavailable` instead
/// of panicking or hanging.
#[tokio::test]
async fn host_call_without_services_is_host_unavailable() {
    let plugin = RunningPlugin::spawn_with(fixture_path(), None, None)
        .await
        .expect("spawn fixture");
    let resp = plugin
        .call(
            "invoke",
            Some("host.say"),
            Some(json!({
                "channel_id": 1,
                "content": "nope",
            })),
        )
        .await
        .expect("host.say invoke answered");
    plugin.stop().await.expect("stop fixture");

    match resp {
        Msg::Resp {
            ok: false,
            error: Some(err),
            ..
        } => assert_eq!(err.kind, "HostUnavailable"),
        other => panic!("expected HostUnavailable resp, got {other:?}"),
    }
}

/// Concurrent plugin→host calls are correlated by id: each resp lands on its
/// own waiting call, never mixed up.
#[tokio::test]
async fn concurrent_host_calls_correlate_by_id() {
    let mut mock = MockHostIo::new();
    mock.expect_send_message()
        .with(
            mockall::predicate::eq(1_u64),
            mockall::predicate::eq("one"),
            mockall::predicate::eq(None::<serde_json::Value>),
        )
        .times(1)
        .returning(|_, _, _| Ok(Some(json!({ "message_id": 1 }))));
    mock.expect_send_message()
        .with(
            mockall::predicate::eq(2_u64),
            mockall::predicate::eq("two"),
            mockall::predicate::eq(None::<serde_json::Value>),
        )
        .times(1)
        .returning(|_, _, _| Ok(Some(json!({ "message_id": 2 }))));

    let plugin = Arc::new(
        RunningPlugin::spawn_with(
            fixture_path(),
            Some(host_services(Arc::new(mock), None)),
            None,
        )
        .await
        .expect("spawn fixture"),
    );
    let (one, two) = tokio::join!(
        plugin.call(
            "invoke",
            Some("host.say"),
            Some(json!({ "channel_id": 1, "content": "one" })),
        ),
        plugin.call(
            "invoke",
            Some("host.say"),
            Some(json!({ "channel_id": 2, "content": "two" })),
        ),
    );
    plugin.stop().await.expect("stop fixture");

    match one.expect("first call answered") {
        Msg::Resp {
            ok: true,
            data: Some(data),
            ..
        } => assert_eq!(data, json!({ "message_id": 1 })),
        other => panic!("expected first resp, got {other:?}"),
    }
    match two.expect("second call answered") {
        Msg::Resp {
            ok: true,
            data: Some(data),
            ..
        } => assert_eq!(data, json!({ "message_id": 2 })),
        other => panic!("expected second resp, got {other:?}"),
    }
}

/// The seam mock must be driven while the fixture waits on its call; the
/// spawn keeps the reader task alive exactly as long as the plugin, so a
/// dropped `RunningPlugin` must not be observable here. Guard against the
/// reader answering after the plugin stopped: a call after `stop` fails fast.
#[tokio::test]
async fn call_after_stop_fails_fast() {
    let mock = MockHostIo::new();
    let plugin = RunningPlugin::spawn_with(
        fixture_path(),
        Some(host_services(Arc::new(mock), None)),
        None,
    )
    .await
    .expect("spawn fixture");
    plugin.stop().await.expect("stop fixture");

    let err = plugin
        .call(
            "invoke",
            Some("host.say"),
            Some(json!({ "channel_id": 1, "content": "late" })),
        )
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("not running"), "err: {msg}");
}

/// The `host.kvget` invoke issues a plugin→host `host.kv.get` call; the host
/// serves it through the mocked KV seam and the fixture echoes the value back
/// to the original invoke.
#[tokio::test]
async fn host_kvget_invoke_serves_kv_get_through_the_seam() {
    let mut mock_kv = MockKvStore::new();
    mock_kv
        .expect_get()
        .with(
            mockall::predicate::eq("settings"),
            mockall::predicate::eq("theme"),
        )
        .times(1)
        .returning(|_, _| Ok(Some("dark".into())));
    let mock_io = MockHostIo::new();
    let plugin = RunningPlugin::spawn_with(
        fixture_path(),
        Some(host_services(Arc::new(mock_io), Some(Arc::new(mock_kv)))),
        None,
    )
    .await
    .expect("spawn fixture");
    let resp = plugin
        .call(
            "invoke",
            Some("host.kvget"),
            Some(json!({ "namespace": "settings", "key": "theme" })),
        )
        .await
        .expect("host.kvget invoke answered");
    plugin.stop().await.expect("stop fixture");

    match resp {
        Msg::Resp {
            id,
            ok: true,
            data: Some(data),
            error: None,
        } => {
            assert_eq!(data, json!({ "value": "dark" }));
            assert_eq!(id, 0);
        }
        other => panic!("expected ok resp echoing the kv value, got {other:?}"),
    }
}

/// The `host.kvset` invoke issues a plugin→host `host.kv.set` call; the host
/// serves it through the mocked KV seam and the fixture forwards the resp (no
/// data) to the original invoke.
#[tokio::test]
async fn host_kvset_invoke_serves_kv_set_through_the_seam() {
    let mut mock_kv = MockKvStore::new();
    mock_kv
        .expect_set()
        .with(
            mockall::predicate::eq("settings"),
            mockall::predicate::eq("theme"),
            mockall::predicate::eq("light"),
        )
        .times(1)
        .returning(|_, _, _| Ok(()));
    let mock_io = MockHostIo::new();
    let plugin = RunningPlugin::spawn_with(
        fixture_path(),
        Some(host_services(Arc::new(mock_io), Some(Arc::new(mock_kv)))),
        None,
    )
    .await
    .expect("spawn fixture");
    let resp = plugin
        .call(
            "invoke",
            Some("host.kvset"),
            Some(json!({
                "namespace": "settings",
                "key": "theme",
                "value": "light",
            })),
        )
        .await
        .expect("host.kvset invoke answered");
    plugin.stop().await.expect("stop fixture");

    match resp {
        Msg::Resp {
            id,
            ok: true,
            data: None,
            error: None,
        } => assert_eq!(id, 0),
        other => panic!("expected ok resp with no data, got {other:?}"),
    }
}

/// The `host.kvdel` invoke issues a plugin→host `host.kv.delete` call; the
/// host serves it through the mocked KV seam and the fixture forwards the
/// resp (no data) to the original invoke.
#[tokio::test]
async fn host_kvdel_invoke_serves_kv_delete_through_the_seam() {
    let mut mock_kv = MockKvStore::new();
    mock_kv
        .expect_delete()
        .with(
            mockall::predicate::eq("settings"),
            mockall::predicate::eq("theme"),
        )
        .times(1)
        .returning(|_, _| Ok(()));
    let mock_io = MockHostIo::new();
    let plugin = RunningPlugin::spawn_with(
        fixture_path(),
        Some(host_services(Arc::new(mock_io), Some(Arc::new(mock_kv)))),
        None,
    )
    .await
    .expect("spawn fixture");
    let resp = plugin
        .call(
            "invoke",
            Some("host.kvdel"),
            Some(json!({ "namespace": "settings", "key": "theme" })),
        )
        .await
        .expect("host.kvdel invoke answered");
    plugin.stop().await.expect("stop fixture");

    match resp {
        Msg::Resp {
            id,
            ok: true,
            data: None,
            error: None,
        } => assert_eq!(id, 0),
        other => panic!("expected ok resp with no data, got {other:?}"),
    }
}
