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

use poise::serenity_prelude as serenity;
use pwr_bot::plugin::HostConfig;
use pwr_bot::plugin::HostIo;
use pwr_bot::plugin::HostServices;
use pwr_bot::plugin::InteractionEngine;
use pwr_bot::plugin::KvStore;
use pwr_bot::plugin::PluginManager;
use pwr_bot::plugin::RespawnPolicy;
use pwr_bot::plugin::RunningPlugin;
use pwr_bot::plugin::StatsHandle;
use pwr_bot::plugin::host::MockHostIo;
use pwr_bot::plugin::host::MockKvStore;
use pwr_plugin_protocol::Msg;
use serde_json::json;

mod probe;
use probe::probe_binary;

fn host_services(io: Arc<dyn HostIo>, kv: Option<Arc<dyn KvStore>>) -> Arc<HostServices> {
    Arc::new(HostServices {
        io: Some(io),
        config: Some(HostConfig {
            db_url: "postgres://test".into(),
            data_path: PathBuf::from("/tmp/pwr-bot-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv,
        engine: None,
        stats: Arc::new(StatsHandle::default()),
        feeds: None,
        voice: None,
    })
}

/// Like [`host_services`], but with the interaction engine wired in so
/// `host.open_view` sessions can be opened.
fn view_host_services(
    io: Arc<dyn HostIo>,
    engine: InteractionEngine<RunningPlugin>,
) -> Arc<HostServices> {
    Arc::new(HostServices {
        io: Some(io),
        config: Some(HostConfig {
            db_url: "postgres://test".into(),
            data_path: PathBuf::from("/tmp/pwr-bot-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv: None,
        engine: Some(Arc::new(engine)),
        stats: Arc::new(StatsHandle::default()),
        feeds: None,
        voice: None,
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
        )
        .times(1)
        .returning(|_, _| Ok(Some(json!({ "message_id": 123_456_789 }))));

    let plugin = RunningPlugin::spawn_with(
        probe_binary("hello"),
        Some(host_services(Arc::new(mock), None)),
        None,
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
        probe_binary("hello"),
        Some(host_services(Arc::new(mock), None)),
        None,
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
            mockall::predicate::eq(json!({
                "components": [{ "type": 10, "content": "edited" }]
            })),
        )
        .times(1)
        .returning(|_, _, _| Ok(Some(json!({ "message_id": 111_222_333 }))));

    let plugin = RunningPlugin::spawn_with(
        probe_binary("hello"),
        Some(host_services(Arc::new(mock), None)),
        None,
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
                "data": { "components": [{ "type": 10, "content": "edited" }] },
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

/// The `host.openview` invoke opens the target plugin's view end to end: the
/// host resolves the target through the manager, posts a placeholder through
/// the io seam, renders the target's panel into an interaction-engine session
/// on the produced message id, then edits the placeholder to the final
/// payload. The resp carries the produced message id, and a subsequent
/// interaction routes through that session back into the target plugin.
#[tokio::test]
async fn host_openview_opens_the_target_plugin_view_end_to_end() {
    let channel_id = 987_654_321_u64;
    let produced = 123_456_789_u64;
    let mut mock = MockHostIo::new();
    mock.expect_send_message()
        .with(
            mockall::predicate::eq(channel_id),
            mockall::predicate::eq("Loading…"),
        )
        .times(1)
        .returning(move |_, _| Ok(Some(json!({ "message_id": produced }))));
    mock.expect_edit_message()
        .with(
            mockall::predicate::eq(channel_id),
            mockall::predicate::eq(produced),
            mockall::predicate::function(|data: &serde_json::Value| {
                // The arg-echo fixture renders a Components V2 payload: the
                // echoed args are the text of a text display. The edit
                // transport strips the create-only fields the fixture's
                // envelope carries (`sticker_ids`, `tts`, `enforce_nonce`):
                // Discord rejects them on edit.
                data == &json!({
                    "attachments": [],
                    "components": [{"content": "{}", "type": 10}],
                    "embeds": [],
                    "flags": 32768,
                })
            }),
        )
        .times(1)
        .returning(move |_, _, _| Ok(Some(json!({ "message_id": produced }))));

    let engine = InteractionEngine::new();
    let services = view_host_services(Arc::new(mock), engine.clone());
    let manager = Arc::new(PluginManager::new(None, RespawnPolicy::default()));
    manager
        .spawn("arg-echo", probe_binary("arg_echo_plugin"), None, &[], &[])
        .await
        .expect("spawn target plugin");

    let caller = RunningPlugin::spawn_with(
        probe_binary("hello"),
        Some(services),
        Some(manager.clone()),
        None,
    )
    .await
    .expect("spawn caller plugin");
    let resp = caller
        .call(
            "invoke",
            Some("host.openview"),
            Some(json!({
                "channel_id": channel_id,
                "plugin": "arg-echo",
                "command": "arg-echo",
                "args": {},
            })),
        )
        .await
        .expect("host.openview invoke answered");
    caller.stop().await.expect("stop caller");

    match resp {
        Msg::Resp {
            id,
            ok: true,
            data: Some(data),
            error: None,
        } => {
            assert_eq!(data, json!({ "message_id": produced }));
            assert_eq!(id, 0);
        }
        other => panic!("expected ok resp echoing the produced message id, got {other:?}"),
    }

    let message_id = serenity::MessageId::new(produced);
    assert!(
        engine.has_session(message_id).await,
        "the produced message has an open session"
    );
    let follow_up = engine
        .interact(message_id, "arg-echo", json!({}))
        .await
        .expect("follow-up interaction routes to the target plugin");
    assert_eq!(
        follow_up.data["components"][0]["content"],
        "{\"custom_id\":\"arg-echo\",\"view\":{\"last_args\":{}}}"
    );
    assert_eq!(follow_up.data["tts"], false);
    assert_eq!(follow_up.data["enforce_nonce"], false);
    manager
        .unload("arg-echo", &[])
        .await
        .expect("stop target plugin");
}

#[tokio::test]
async fn host_openview_rejects_malformed_view_before_sending_or_registering() {
    let mut mock = MockHostIo::new();
    mock.expect_send_message().times(0);
    mock.expect_edit_message().times(0);

    let engine = InteractionEngine::new();
    let services = view_host_services(Arc::new(mock), engine.clone());
    let manager = Arc::new(PluginManager::new(None, RespawnPolicy::default()));
    manager
        .spawn("arg-echo", probe_binary("arg_echo_plugin"), None, &[], &[])
        .await
        .expect("spawn target plugin");
    let caller = RunningPlugin::spawn_with(
        probe_binary("hello"),
        Some(services),
        Some(manager.clone()),
        None,
    )
    .await
    .expect("spawn caller plugin");

    let resp = caller
        .call(
            "invoke",
            Some("host.openview"),
            Some(json!({
                "channel_id": 987_654_321_u64,
                "plugin": "arg-echo",
                "command": "malformed",
                "args": {},
            })),
        )
        .await
        .expect("host.openview invoke answered");
    caller.stop().await.expect("stop caller");

    let Msg::Resp {
        ok: false,
        error: Some(error),
        ..
    } = resp
    else {
        panic!("expected InvalidView response, got {resp:?}");
    };
    assert_eq!(error.kind, "InvalidView");
    assert_eq!(engine.session_count().await, 0);
    assert!(
        !engine
            .has_session(serenity::MessageId::new(987_654_321))
            .await
    );
    manager
        .unload("arg-echo", &[])
        .await
        .expect("stop target plugin");
}

/// Without services wired, a `host.*` call answers `HostUnavailable` instead
/// of panicking or hanging.
#[tokio::test]
async fn host_call_without_services_is_host_unavailable() {
    let plugin = RunningPlugin::spawn_with(probe_binary("hello"), None, None, None)
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
        .with(mockall::predicate::eq(1_u64), mockall::predicate::eq("one"))
        .times(1)
        .returning(|_, _| Ok(Some(json!({ "message_id": 1 }))));
    mock.expect_send_message()
        .with(mockall::predicate::eq(2_u64), mockall::predicate::eq("two"))
        .times(1)
        .returning(|_, _| Ok(Some(json!({ "message_id": 2 }))));

    let plugin = Arc::new(
        RunningPlugin::spawn_with(
            probe_binary("hello"),
            Some(host_services(Arc::new(mock), None)),
            None,
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
        probe_binary("hello"),
        Some(host_services(Arc::new(mock), None)),
        None,
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
        probe_binary("hello"),
        Some(host_services(Arc::new(mock_io), Some(Arc::new(mock_kv)))),
        None,
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
        probe_binary("hello"),
        Some(host_services(Arc::new(mock_io), Some(Arc::new(mock_kv)))),
        None,
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
        probe_binary("hello"),
        Some(host_services(Arc::new(mock_io), Some(Arc::new(mock_kv)))),
        None,
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

/// The fixture's `host.openmodal` invoke issues a plugin→host
/// `host.open_modal` call through the real stdio wire; the host opens the
/// modal through the mocked seam and binds the author route to the calling
/// session. A later submission from that author rides the route back to the
/// fixture as a `view.modal_submit` call, and the fixture's echo view is the
/// delivered spec. The route is one-shot: the second submission finds none.
#[tokio::test]
async fn open_modal_round_trips_the_submission_to_the_owning_session() {
    let modal = json!({
        "custom_id": "hello:modal",
        "title": "Tell us",
        "components": [{
            "type": 18,
            "label": "Note",
            "component": {
                "type": 4,
                "style": 1,
                "custom_id": "note",
                "min_length": null,
                "max_length": null,
                "required": true
            }
        }]
    });
    let mut mock = MockHostIo::new();
    mock.expect_open_modal()
        .with(
            mockall::predicate::eq(42_u64),
            mockall::predicate::eq("token-1"),
            mockall::predicate::eq(modal.clone()),
        )
        .times(1)
        .returning(|_, _, _| Ok(()));
    let manager = Arc::new(
        PluginManager::new(None, RespawnPolicy::default())
            .with_host_services(host_services(Arc::new(mock), None)),
    );
    let plugin = manager
        .spawn("hello", probe_binary("hello"), None, &[], &[])
        .await
        .expect("spawn fixture");

    let resp = plugin
        .call(
            "invoke",
            Some("host.openmodal"),
            Some(json!({
                "author_id": 7,
                "interaction_id": 42,
                "token": "token-1",
                "modal": modal
            })),
        )
        .await
        .expect("host.openmodal invoke answered");
    match resp {
        Msg::Resp {
            id,
            ok: true,
            data: None,
            error: None,
        } => assert_eq!(id, 0),
        other => panic!("expected ok resp with no data, got {other:?}"),
    }

    // The author submits: the raw ModalInteraction JSON rides the route to
    // the fixture, which echoes the `note` input's value as its view.
    let submission = json!({
        "id": "999",
        "token": "submit-token",
        "application_id": "1",
        "type": 5,
        "data": {
            "custom_id": "hello:modal",
            "components": [{
                "type": 18,
                "label": "Note",
                "component": {
                    "type": 4,
                    "style": 1,
                    "custom_id": "note",
                    "min_length": null,
                    "max_length": null,
                    "required": true,
                    "value": "hi there"
                }
            }]
        }
    });
    let spec = manager
        .deliver_modal_submission(7, submission)
        .await
        .expect("submission delivered to the owning session");
    assert_eq!(
        spec.data["components"][0]["content"],
        serde_json::json!("Modal submitted! note=hi there"),
        "the fixture's echo view is the delivered spec"
    );

    // One-shot: the consumed route finds no binding for a second submission.
    let error = manager
        .deliver_modal_submission(7, json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            pwr_bot::plugin::ModalDeliveryError::Route(
                pwr_bot::plugin::ModalRouteError::NoBinding(7)
            )
        ),
        "second submission must not deliver: {error}"
    );

    manager.unload("hello", &[]).await.expect("unload fixture");
}

/// Unloading the owner drops its modal routes: a submission after the
/// unload finds no binding and falls back to the message-keyed route
/// instead of hanging or delivering to a dead session.
#[tokio::test]
async fn unloading_the_owner_drops_its_modal_routes() {
    let mut mock = MockHostIo::new();
    mock.expect_open_modal()
        .times(1)
        .returning(|_, _, _| Ok(()));
    let manager = Arc::new(
        PluginManager::new(None, RespawnPolicy::default())
            .with_host_services(host_services(Arc::new(mock), None)),
    );
    let plugin = manager
        .spawn("hello", probe_binary("hello"), None, &[], &[])
        .await
        .expect("spawn fixture");
    plugin
        .call(
            "invoke",
            Some("host.openmodal"),
            Some(json!({
                "author_id": 7,
                "interaction_id": 42,
                "token": "token-1",
                "modal": {
                    "custom_id": "hello:modal",
                    "title": "Tell us",
                    "components": []
                }
            })),
        )
        .await
        .expect("host.openmodal invoke answered");

    manager.unload("hello", &[]).await.expect("unload fixture");

    let error = manager
        .deliver_modal_submission(7, json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            pwr_bot::plugin::ModalDeliveryError::Route(
                pwr_bot::plugin::ModalRouteError::NoBinding(7)
            )
        ),
        "the route must die with the session: {error}"
    );
}
