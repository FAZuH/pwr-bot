//! Integration tests for the interaction engine: drive a real `hello_plugin`
//! fixture subprocess through [`InteractionEngine`] and assert the full
//! invoke → click → timeout lifecycle. Pure stdio — no database.
//!
//! A separate test binary from `plugin_manager.rs` so each process owns its
//! recording-logger static without interference.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Once;
use std::sync::OnceLock;
use std::time::Duration;

use log::LevelFilter;
use log::Log;
use poise::serenity_prelude as serenity;
use pwr_bot::plugin::InteractionEngine;
use pwr_bot::plugin::InteractionError;
use pwr_bot::plugin::RunningPlugin;
use pwr_plugin_protocol::BUTTON_CUSTOM_ID;
use pwr_plugin_protocol::PLUGIN_NAME;
use pwr_plugin_protocol::ViewSpec;
use serde_json::Value;
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

/// Polls `cond` until it is true or `timeout` elapses.
async fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    cond()
}

/// Reads the click count from a fixture view's content string.
fn parse_count(spec: &ViewSpec) -> u64 {
    spec.data["content"]
        .as_str()
        .expect("content string")
        .split("count=")
        .nth(1)
        .expect("count present")
        .parse()
        .expect("count parses")
}

/// Spawns the fixture plugin and opens a session on a fresh engine for
/// `message_id`, returning the plugin (owned by the caller, stopped at the
/// end of the test), the engine, and the rendered spec.
async fn spawn_engine_and_open(
    message_id: serenity::MessageId,
) -> (
    Arc<RunningPlugin>,
    InteractionEngine<RunningPlugin>,
    ViewSpec,
) {
    let plugin = Arc::new(
        RunningPlugin::spawn(fixture_path())
            .await
            .expect("spawn hello_plugin"),
    );
    let engine = InteractionEngine::new();
    let spec = engine
        .open(message_id, plugin.clone(), PLUGIN_NAME, json!({}))
        .await
        .expect("open the view");
    (plugin, engine, spec)
}

// ── log capture: the host runtime logs through the `log` crate ─────────────

static LOG_INIT: Once = Once::new();
static LOGS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

struct RecordingLogger;

impl Log for RecordingLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if let Some(logs) = LOGS.get() {
            logs.lock().unwrap().push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

/// Installs a process-wide recording logger (once), so tests can assert on
/// what the plugin runtime logged.
fn init_recording_logger() {
    LOG_INIT.call_once(|| {
        let _ = LOGS.set(Mutex::new(Vec::new()));
        let _ = log::set_boxed_logger(Box::new(RecordingLogger));
        log::set_max_level(LevelFilter::Info);
    });
}

fn logs_contain(fragment: &str) -> bool {
    LOGS.get()
        .map(|logs| {
            logs.lock()
                .unwrap()
                .iter()
                .any(|line| line.contains(fragment))
        })
        .unwrap_or(false)
}

// ── the full invoke → click → timeout lifecycle ────────────────────────────

#[tokio::test]
async fn engine_drives_the_fixture_view_lifecycle() {
    init_recording_logger();
    if let Some(logs) = LOGS.get() {
        logs.lock().unwrap().clear();
    }

    let message_id = serenity::MessageId::new(1);
    let (plugin, engine, spec) = spawn_engine_and_open(message_id).await;
    assert_eq!(spec.data["content"], "Hello from plugin!");
    assert_eq!(
        spec.data["components"][0]["components"][0]["custom_id"], BUTTON_CUSTOM_ID,
        "the fixture's button rides in the rendered spec"
    );
    assert!(!spec.ephemeral);
    assert!(
        engine.has_session(message_id).await,
        "the session is open after render"
    );
    assert_eq!(
        engine.view_state(message_id).await,
        Some(Value::Null),
        "the fixture serves the raw v1 shape: no envelope view state"
    );

    // First click: view.interact round trip through the fixture.
    let spec = engine
        .interact(message_id, BUTTON_CUSTOM_ID, json!({}))
        .await
        .expect("first click");
    assert!(
        spec.data["content"]
            .as_str()
            .expect("content")
            .contains("count=1")
    );

    // Second click: the fixture's own counter, not a host-side one.
    let spec = engine
        .interact(message_id, BUTTON_CUSTOM_ID, json!({}))
        .await
        .expect("second click");
    assert!(
        spec.data["content"]
            .as_str()
            .expect("content")
            .contains("count=2")
    );

    // Abandonment pushes view.timeout (one-way) and drops the session.
    engine.abandon(message_id).await.expect("abandon the view");
    assert!(
        !engine.has_session(message_id).await,
        "the session is dropped on abandon"
    );
    let timed_out = wait_until(Duration::from_secs(5), || {
        logs_contain("event: view.timeout received")
    })
    .await;
    assert!(timed_out, "the plugin never saw the view.timeout event");

    plugin.stop().await.expect("graceful stop");
}

// ── wire error surfacing ───────────────────────────────────────────────────

#[tokio::test]
async fn engine_surfaces_unknown_custom_id_from_the_fixture() {
    let message_id = serenity::MessageId::new(1);
    let (plugin, engine, _) = spawn_engine_and_open(message_id).await;

    let err = engine
        .interact(message_id, "hello:nope", json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(
            &err,
            InteractionError::PluginRejected { kind, msg }
                if kind == "UnknownAction" && msg == "unknown custom_id"
        ),
        "expected a plugin rejection, got {err:?}"
    );
    assert!(
        engine.has_session(message_id).await,
        "a rejected interaction keeps the session"
    );

    plugin.stop().await.expect("graceful stop");
}

// ── modal submit routing ───────────────────────────────────────────────────

#[tokio::test]
async fn modal_submit_round_trips_through_the_fixture() {
    let message_id = serenity::MessageId::new(1);
    let (plugin, engine, _) = spawn_engine_and_open(message_id).await;

    // A Discord modal submit arrives as a view.interact with the modal's
    // custom id and the component values in the payload.
    let spec = engine
        .interact(
            message_id,
            "hello:modal",
            json!({
                "custom_id": "hello:modal",
                "components": [{"type": 4, "custom_id": "note", "value": "hi"}],
            }),
        )
        .await
        .expect("modal submit");
    assert!(
        spec.data["content"]
            .as_str()
            .expect("content")
            .contains("Modal submitted! count=1")
    );
    assert!(
        engine.has_session(message_id).await,
        "the session survives the modal submit"
    );

    plugin.stop().await.expect("graceful stop");
}

// ── concurrent sessions ────────────────────────────────────────────────────

#[tokio::test]
async fn concurrent_sessions_route_to_their_own_responses() {
    let a = serenity::MessageId::new(1);
    let b = serenity::MessageId::new(2);
    let (plugin, engine, _) = spawn_engine_and_open(a).await;
    engine
        .open(b, plugin.clone(), PLUGIN_NAME, json!({}))
        .await
        .expect("open b");

    // Fire all three clicks before any is answered; the fixture answers
    // serially, so only per-message routing plus call-id correlation resolve
    // each click to its own count.
    let (c1, c2, c3) = tokio::join!(
        engine.interact(a, BUTTON_CUSTOM_ID, json!({})),
        engine.interact(b, BUTTON_CUSTOM_ID, json!({})),
        engine.interact(a, BUTTON_CUSTOM_ID, json!({})),
    );
    let mut counts: Vec<u64> = [c1, c2, c3]
        .map(|r| r.expect("click resolves"))
        .map(|spec| parse_count(&spec))
        .to_vec();
    counts.sort_unstable();
    assert_eq!(counts, vec![1, 2, 3], "each click sees its own counter");
    assert!(
        engine.has_session(a).await && engine.has_session(b).await,
        "both sessions survive the concurrent clicks"
    );

    plugin.stop().await.expect("graceful stop");
}
