//! Integration tests for the Discord-event fan-out: drive a real
//! `hello` fixture subprocess through [`PluginEventRouter`] and assert
//! that (T1) a subscribed plugin receives a fanned-out Discord event, and
//! (T2) the plugin's plugin→host event is broadcast on the host event bus.
//! Pure stdio — no database.
//!
//! A separate test binary from `plugin_interaction.rs` so each process owns
//! its recording-logger static without interference.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Once;
use std::sync::OnceLock;
use std::time::Duration;

use log::LevelFilter;
use log::Log;
use pwr_bot::event::PluginEvent;
use pwr_bot::event::event_bus::EventBus;
use pwr_bot::plugin::PluginEventRouter;
use pwr_bot::plugin::PluginManager;
use pwr_bot::plugin::RespawnPolicy;
use pwr_bot::plugin::VOICE_STATE_EVENT;
use pwr_plugin_protocol::PLUGIN_NAME;
use serde_json::json;

/// Locates the `hello` fixture binary. `CARGO_BIN_EXE_hello`
/// is set by cargo for the hello crate's own tests; for host-crate tests
/// the workspace build places the binary under `target/{profile}`. The test
/// binary does not expose the active profile, so probe `debug` and `release`
/// instead of guessing: CI (`cargo build --all-targets`) and local
/// `cargo build --workspace` both land the fixture in one of the two.
/// A missing binary panics with a build hint rather than a confusing spawn
/// error.
fn fixture_path() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_hello") {
        return PathBuf::from(path);
    }
    let target = match option_env!("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"),
    };
    for profile in ["debug", "release"] {
        let candidate = target.join(profile).join("hello");
        if candidate.exists() {
            return candidate;
        }
    }
    panic!(concat!(
        "test-plugin fixture not built; run `cargo build -p hello` ",
        "(or `cargo build --workspace`) first"
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

// ── T1: subscribed plugin receives a fanned-out Discord event ──────────────

#[tokio::test]
async fn subscribed_plugin_receives_a_fanned_out_discord_event() {
    init_recording_logger();
    if let Some(logs) = LOGS.get() {
        logs.lock().unwrap().clear();
    }

    let router = Arc::new(PluginEventRouter::new());
    let manager = Arc::new(
        PluginManager::new(None, RespawnPolicy::default()).with_event_router(router.clone()),
    );
    let plugin = manager
        .spawn(
            PLUGIN_NAME,
            fixture_path(),
            None,
            &[VOICE_STATE_EVENT.to_string()],
            &[],
        )
        .await
        .expect("spawn hello_plugin");

    router
        .fan_out(&manager, VOICE_STATE_EVENT, &json!({"user_id": "123"}))
        .await;

    let received = wait_until(Duration::from_secs(5), || {
        logs_contain("event: voice_state received")
    })
    .await;
    assert!(
        received,
        "the subscribed plugin never saw the fanned-out voice_state event"
    );

    plugin.stop().await.expect("graceful stop");
}

// ── T2: plugin→host event is broadcast on the host event bus ───────────────

#[tokio::test]
async fn plugin_event_is_broadcast_on_the_host_event_bus() {
    let bus = Arc::new(EventBus::new());
    let seen = Arc::new(Mutex::new(Vec::<PluginEvent>::new()));
    bus.register_callback({
        let seen = seen.clone();
        move |event: PluginEvent| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(event);
                Ok(())
            }
        }
    });

    let router = Arc::new(PluginEventRouter::new());
    let manager = Arc::new(
        PluginManager::new(None, RespawnPolicy::default())
            .with_event_bus(bus.clone())
            .with_event_router(router.clone()),
    );
    let plugin = manager
        .spawn(
            PLUGIN_NAME,
            fixture_path(),
            None,
            &[VOICE_STATE_EVENT.to_string()],
            &[],
        )
        .await
        .expect("spawn hello_plugin");

    router
        .fan_out(&manager, VOICE_STATE_EVENT, &json!({"user_id": "42"}))
        .await;

    let saw_ack = wait_until(Duration::from_secs(5), || {
        seen.lock()
            .unwrap()
            .iter()
            .any(|event| event.plugin == PLUGIN_NAME && event.name == "voice_state.ack")
    })
    .await;
    assert!(
        saw_ack,
        "the plugin's voice_state.ack never reached the bus"
    );

    let events = seen.lock().unwrap();
    let ack = events
        .iter()
        .find(|event| event.name == "voice_state.ack")
        .expect("ack present");
    assert_eq!(ack.plugin, PLUGIN_NAME);
    assert_eq!(ack.name, "voice_state.ack");
    assert_eq!(ack.data, Some(json!({"user_id": "42"})));

    plugin.stop().await.expect("graceful stop");
}
