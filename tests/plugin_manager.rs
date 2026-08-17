//! Integration tests for the plugin runtime: spawn the real fixture binary
//! (`hello_plugin`) through [`RunningPlugin`] and drive the wire protocol end
//! to end. Pure stdio — no database.

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::Once;
use std::sync::OnceLock;
use std::time::Duration;

use log::LevelFilter;
use log::Log;
use pwr_bot::plugin::PluginError;
use pwr_bot::plugin::RunningPlugin;
use pwr_plugin_protocol::BUTTON_CUSTOM_ID;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::PLUGIN_NAME;
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

/// Absolute path to a test-only shell fixture in `tests/fixtures/`.
fn fixture_script(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
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

// ── spawn -> hello -> invoke -> resp -> bye ────────────────────────────────

#[tokio::test]
async fn spawn_hello_invoke_bye_round_trip() {
    let plugin = RunningPlugin::spawn(fixture_path())
        .await
        .expect("spawn hello_plugin");

    // The handshake (hello -> validate -> host ack) ran inside spawn; the
    // first call proves the plugin is up and answering.
    let resp = plugin
        .call("invoke", Some(PLUGIN_NAME), Some(json!({})))
        .await
        .expect("invoke call");
    let Msg::Resp {
        ok, data, error, ..
    } = resp
    else {
        panic!("expected resp, got {resp:?}");
    };
    assert!(ok, "invoke must succeed");
    assert!(error.is_none(), "invoke must carry no error");
    assert_eq!(data.expect("invoke data")["content"], "Hello from plugin!");

    let status = plugin.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}

// ── call/resp correlation ──────────────────────────────────────────────────

#[tokio::test]
async fn concurrent_calls_resolve_to_their_own_resps() {
    let plugin = RunningPlugin::spawn(fixture_path())
        .await
        .expect("spawn hello_plugin");

    // Fire both before either is answered; the fixture answers serially, so
    // only id-based correlation resolves each caller to its own resp.
    let (invoke, interact) = tokio::join!(
        plugin.call("invoke", Some(PLUGIN_NAME), Some(json!({}))),
        plugin.call(
            "view.interact",
            Some(PLUGIN_NAME),
            Some(json!({
                "custom_id": BUTTON_CUSTOM_ID,
                "values": [],
                "user_id": 123,
                "channel_id": 456,
                "guild_id": 789
            })),
        ),
    );
    let invoke = invoke.expect("invoke call");
    let interact = interact.expect("interact call");

    let Msg::Resp {
        id: invoke_id,
        data: invoke_data,
        ..
    } = invoke
    else {
        panic!("expected resp, got {invoke:?}");
    };
    let Msg::Resp {
        id: interact_id,
        data: interact_data,
        ..
    } = interact
    else {
        panic!("expected resp, got {interact:?}");
    };
    assert_ne!(invoke_id, interact_id, "calls get distinct correlation ids");
    assert_eq!(
        invoke_data.expect("invoke data")["content"],
        "Hello from plugin!"
    );
    assert!(
        interact_data.expect("interact data")["content"]
            .as_str()
            .expect("content string")
            .contains("clicked"),
        "interact resp must belong to the interact call"
    );

    plugin.stop().await.expect("stop");
}

// ── stderr forwarding ──────────────────────────────────────────────────────

#[tokio::test]
async fn plugin_stderr_reaches_host_logs() {
    init_recording_logger();
    if let Some(logs) = LOGS.get() {
        logs.lock().unwrap().clear();
    }

    let plugin = RunningPlugin::spawn(fixture_script("crash_plugin.sh"))
        .await
        .expect("spawn crash fixture");

    // The fixture announces itself on stderr right after its hello; the host
    // must forward that line into its own logs. Asserting on this real line
    // keeps the test independent of how the fixture reacts to the host's
    // hello ack (the fixture now tolerates it silently).
    let forwarded = wait_until(Duration::from_secs(5), || {
        logs_contain("crash_plugin: starting")
    })
    .await;
    assert!(forwarded, "plugin stderr line did not reach the host logs");

    // The fixture killed itself after the handshake; stop() observes the
    // reaper's recorded status instead of blocking on a dead process.
    let _ = plugin.stop().await;
}

// ── clean exit vs crash (exit status) ──────────────────────────────────────

#[tokio::test]
async fn unresponsive_plugin_is_killed_on_stop() {
    let plugin = RunningPlugin::spawn(fixture_script("stubborn_plugin.sh"))
        .await
        .expect("spawn stubborn fixture");

    // The fixture ignores bye and stdin EOF; stop() must SIGTERM it after
    // the grace period, and the status must reflect a signal death, not
    // exit 0.
    let status = plugin.stop().await.expect("stop kills the plugin");
    assert_eq!(
        status.code(),
        None,
        "signal death has no exit code: {status}"
    );
    #[cfg(unix)]
    assert_eq!(status.signal(), Some(15), "killed with SIGTERM: {status}");
}

#[tokio::test]
async fn in_flight_call_fails_with_plugin_died_on_crash() {
    let plugin = RunningPlugin::spawn(fixture_script("crash_plugin.sh"))
        .await
        .expect("spawn crash fixture");

    // The fixture kills itself the moment it reads the call, before answering.
    let resp = plugin
        .call("invoke", Some(PLUGIN_NAME), None)
        .await
        .expect("call while the plugin dies");
    let Msg::Resp { ok, error, .. } = resp else {
        panic!("expected resp, got {resp:?}");
    };
    assert!(!ok, "crash must fail the in-flight call");
    let error = error.expect("failed resp carries a wire error");
    assert_eq!(error.kind, "PluginDied");

    // The manager knows the plugin is gone: later calls fail fast, and the
    // reaper recorded a signal death.
    let err = plugin
        .call("invoke", Some(PLUGIN_NAME), None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, PluginError::Io { .. }),
        "dead plugin write fails: {err:?}"
    );

    let reaped = wait_until(Duration::from_secs(5), || plugin.exit_status().is_some()).await;
    assert!(reaped, "reaper must record the exit status");
    let status = plugin.exit_status().expect("recorded status");
    assert_eq!(
        status.code(),
        None,
        "signal death has no exit code: {status}"
    );
    #[cfg(unix)]
    assert_eq!(status.signal(), Some(9), "killed with SIGKILL: {status}");
}

#[tokio::test]
async fn plugin_panic_exits_nonzero_with_stderr_diagnostics() {
    init_recording_logger();
    if let Some(logs) = LOGS.get() {
        logs.lock().unwrap().clear();
    }

    let plugin = RunningPlugin::spawn(fixture_path())
        .await
        .expect("spawn hello_plugin");

    // The panic fires before any resp is written, so the reader task fails
    // the in-flight call with the PluginDied wire error (same path as a
    // crash).
    let resp = plugin
        .call("invoke", Some("panic"), None)
        .await
        .expect("call while the plugin panics");
    let Msg::Resp { ok, error, .. } = resp else {
        panic!("expected resp, got {resp:?}");
    };
    assert!(!ok, "panic must fail the in-flight call");
    let error = error.expect("failed resp carries a wire error");
    assert_eq!(error.kind, "PluginDied");

    // catch_unwind converts the panic into a nonzero FAILURE exit, not a
    // signal death, so the reaper records code 1.
    let reaped = wait_until(Duration::from_secs(5), || plugin.exit_status().is_some()).await;
    assert!(reaped, "reaper must record the exit status");
    let status = plugin.exit_status().expect("recorded status");
    assert_eq!(
        status.code(),
        Some(1),
        "panic exits nonzero with FAILURE: {status}"
    );
    #[cfg(unix)]
    assert_eq!(
        status.signal(),
        None,
        "panic must not be a signal death: {status}"
    );

    // The stderr diagnostics reached the host logs via the forwarding task.
    let forwarded = wait_until(Duration::from_secs(5), || logs_contain("plugin panicked")).await;
    assert!(forwarded, "panic diagnostics did not reach the host logs");
}

// ── handshake rejection ────────────────────────────────────────────────────

#[tokio::test]
async fn spawn_rejects_a_binary_that_never_announces() {
    let err = match RunningPlugin::spawn("/bin/true").await {
        Ok(_) => panic!("a binary that never writes a hello must be rejected"),
        Err(e) => e,
    };
    assert!(
        matches!(err, PluginError::HelloLost { .. }),
        "expected hello-lost rejection, got {err:?}"
    );
}
