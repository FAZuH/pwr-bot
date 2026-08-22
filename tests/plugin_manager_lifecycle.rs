//! Integration tests for the plugin lifecycle: health checks, unload,
//! respawn, and swap driven through [`PluginManager`] with the real fixture
//! binaries. Pure stdio — no database.
//!
//! A separate test binary from `plugin_manager.rs`/`plugin_interaction.rs`
//! so each process owns its state without interference; this binary asserts
//! on behaviour (not logs) and installs no recording logger.

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pwr_bot::event::PluginEvent;
use pwr_bot::event::event_bus::EventBus;
use pwr_bot::plugin::HealthConfig;
use pwr_bot::plugin::PluginManager;
use pwr_bot::plugin::RespawnOutcome;
use pwr_bot::plugin::RespawnPolicy;
use pwr_bot::plugin::RunningPlugin;
use pwr_plugin_protocol::BUTTON_CUSTOM_ID;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::PLUGIN_NAME;
use pwr_plugin_protocol::TaskDef;
use serde_json::json;

mod probe;
use probe::probe_binary;

/// Absolute path to a test-only shell fixture in `tests/fixtures/`.
fn fixture_script(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Base path for the process-group fixtures' pid/marker files. The fixtures
/// derive the same path from their parent's pid (`$PPID`, the test binary)
/// plus a fixture tag, so each test owns a distinct, stable path.
#[cfg(unix)]
fn group_fixture_base(tag: &str) -> PathBuf {
    PathBuf::from(format!(
        "/tmp/pwr_bot_plugin_group_{}_{}",
        std::process::id(),
        tag
    ))
}

/// Reads the child pid the process-group fixtures write, polling until it
/// appears.
#[cfg(unix)]
async fn wait_for_group_child(base: &Path) -> i32 {
    let pid_path = base.with_extension("pid");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(raw) = std::fs::read_to_string(&pid_path)
            && let Ok(pid) = raw.trim().parse()
        {
            return pid;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "fixture never wrote the child pid to {}",
            pid_path.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Whether a process with `pid` exists, via `kill(pid, 0)`: a zero return
/// means it exists, EPERM means it exists (just not ours to signal), and
/// ESRCH means it is gone.
#[cfg(unix)]
fn process_alive(pid: i32) -> bool {
    // SAFETY: `kill(pid, 0)` is a pure existence probe — signal 0 has no
    // effect on a live process — and `pid` came from the fixture.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Polls an async condition until it is true or `timeout` elapses.
async fn wait_until_async<F, Fut>(timeout: Duration, mut cond: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if cond().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cond().await
}

/// Short health config for tests: a 50ms ping interval, 3 missed pongs, and
/// a 20ms pong window.
fn test_health() -> HealthConfig {
    HealthConfig {
        interval: Duration::from_millis(50),
        max_missed_pongs: 3,
        pong_window: Duration::from_millis(20),
    }
}

/// Short respawn policy for tests: 10ms base backoff, 100ms cap, 3 attempts
/// within 2s.
fn test_policy() -> RespawnPolicy {
    RespawnPolicy {
        backoff_base: Duration::from_millis(10),
        backoff_cap: Duration::from_millis(100),
        max_attempts: 3,
        crash_loop_window: Duration::from_secs(2),
    }
}

/// Clicks the fixture's button and reports whether the response carries
/// `count=1` (a fresh process).
async fn click(plugin: &RunningPlugin) -> Result<bool, pwr_bot::plugin::PluginError> {
    let resp = plugin
        .call(
            "view.interact",
            Some(PLUGIN_NAME),
            Some(json!({
                "custom_id": BUTTON_CUSTOM_ID,
                "values": [],
                "user_id": 123,
                "channel_id": 456,
                "guild_id": 789,
            })),
        )
        .await?;
    let Msg::Resp { ok, data, .. } = resp else {
        return Ok(false);
    };
    if !ok {
        return Ok(false);
    }
    let content = data.and_then(|data| data["content"].as_str().map(str::to_string));
    Ok(content.is_some_and(|content| content.contains("count=1")))
}

// ── health: ping/pong keeps a healthy plugin running ───────────────────────

#[tokio::test]
async fn health_pings_keep_a_healthy_plugin_running() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    manager
        .spawn(
            "hello",
            probe_binary("hello"),
            Some(test_health()),
            &[],
            &[],
        )
        .await
        .expect("spawn hello_plugin");

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        manager.is_running("hello").await,
        "healthy plugin stays registered"
    );
    let plugin = manager.get("hello").await.expect("registered handle");
    assert!(
        plugin.pongs_received() > 0,
        "health pings must be answered with pongs"
    );
    let resp = plugin
        .call("invoke", Some(PLUGIN_NAME), Some(json!({})))
        .await
        .expect("call still works");
    assert!(matches!(resp, Msg::Resp { ok: true, .. }));

    manager.unload("hello", &[]).await.expect("teardown");
}

// ── spawn race: the winner keeps its entry ─────────────────────────────────

#[tokio::test]
async fn concurrent_spawn_race_keeps_the_winner() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    let (a, b) = tokio::join!(
        manager.spawn("hello", probe_binary("hello"), None, &[], &[]),
        manager.spawn("hello", probe_binary("hello"), None, &[], &[]),
    );
    let (winner, loser) = match (a, b) {
        (Ok(winner), Err(loser)) => (winner, loser),
        (Err(loser), Ok(winner)) => (winner, loser),
        _ => panic!("expected exactly one winner and one AlreadyRunning loser"),
    };
    assert!(
        matches!(loser, pwr_bot::plugin::PluginError::AlreadyRunning { .. }),
        "loser must report AlreadyRunning, got {loser:?}"
    );
    // The map holds the winner's handle: it is the registered instance and
    // still answers calls.
    let registered = manager.get("hello").await.expect("registered handle");
    assert!(
        Arc::ptr_eq(&registered, &winner),
        "the map must keep the winner's handle"
    );
    let resp = registered
        .call("invoke", Some(PLUGIN_NAME), Some(json!({})))
        .await
        .expect("winner answers");
    assert!(matches!(resp, Msg::Resp { ok: true, .. }));

    manager.unload("hello", &[]).await.expect("teardown");
}

// ── health: a silent plugin is respawned after missed pongs ────────────────

#[tokio::test]
async fn silent_plugin_is_respawned_after_missed_pongs() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    manager
        .spawn(
            "hung",
            fixture_script("hung_plugin.sh"),
            Some(test_health()),
            &[],
            &[],
        )
        .await
        .expect("spawn hung fixture");
    let original = manager.get("hung").await.expect("registered handle");

    // The hung fixture never answers pings; the health task must unload it
    // and respawn a fresh instance.
    let respawned = wait_until_async(Duration::from_secs(5), || {
        let manager = manager.clone();
        let original = original.clone();
        async move {
            let Some(plugin) = manager.get("hung").await else {
                return false;
            };
            !Arc::ptr_eq(&plugin, &original)
        }
    })
    .await;
    assert!(
        respawned,
        "silent plugin must be respawned after missed pongs"
    );

    // Best-effort teardown: the hung fixture exits 0 on bye/EOF, and the
    // entry may already be gone once the crash-loop cap ends the cycle.
    let _ = manager.unload("hung", &[]).await;
}

// ── supervision: kill → respawn serves a fresh instance ───────────────────

#[tokio::test]
async fn killed_plugin_is_respawned_and_serves_fresh_instances() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    manager
        .spawn(
            "hello",
            probe_binary("hello"),
            Some(test_health()),
            &[],
            &[],
        )
        .await
        .expect("spawn hello_plugin");
    let plugin = manager.get("hello").await.expect("registered handle");

    // The fixture keeps a per-process click counter; the first click lands
    // on process #1.
    assert!(
        click(&plugin).await.expect("first click"),
        "count=1 on the first process"
    );

    // SIGKILL the child; the health task must notice the crash and respawn.
    let pid = plugin.pid().await.expect("child pid");
    let killed = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .expect("kill child");
    assert!(killed.success(), "kill -9 must succeed");

    // A fresh instance answers: the click counter is back to 1 (a new
    // process), and only a respawned handle answers at all.
    let respawned = wait_until_async(Duration::from_secs(5), || {
        let manager = manager.clone();
        async move {
            if !manager.is_running("hello").await {
                return false;
            }
            let Some(plugin) = manager.get("hello").await else {
                return false;
            };
            click(&plugin).await.unwrap_or(false)
        }
    })
    .await;
    assert!(respawned, "manager must respawn a killed plugin");

    manager.unload("hello", &[]).await.expect("teardown");
}

// ── supervision: crashes are respawned without a health config ─────────────

#[tokio::test]
async fn killed_plugin_is_respawned_even_without_a_health_config() {
    // Core plugins spawn with health = None (src/bot/mod.rs); supervision
    // must not depend on it.
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    manager
        .spawn("hello", probe_binary("hello"), None, &[], &[])
        .await
        .expect("spawn hello_plugin");
    let plugin = manager.get("hello").await.expect("registered handle");
    assert!(
        click(&plugin).await.expect("first click"),
        "count=1 on the first process"
    );

    // SIGKILL the child; the crash supervisor must notice the death and
    // respawn even though no health task exists.
    let pid = plugin.pid().await.expect("child pid");
    let killed = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .expect("kill child");
    assert!(killed.success(), "kill -9 must succeed");

    let respawned = wait_until_async(Duration::from_secs(5), || {
        let manager = manager.clone();
        async move {
            if !manager.is_running("hello").await {
                return false;
            }
            let Some(plugin) = manager.get("hello").await else {
                return false;
            };
            // Only a fresh process answers with its counter reset to 1; the
            // killed process had already served count=1 before the kill.
            click(&plugin).await.unwrap_or(false)
        }
    })
    .await;
    assert!(
        respawned,
        "a killed plugin must be respawned without a health config"
    );

    manager.unload("hello", &[]).await.expect("teardown");
}

#[tokio::test]
async fn clean_exit_without_a_health_config_is_unloaded_not_respawned() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    manager
        .spawn(
            "clean",
            fixture_script("clean_exit_plugin.sh"),
            None,
            &[],
            &[],
        )
        .await
        .expect("spawn clean-exit fixture");

    // The fixture exits 0 on its own; with no health task running, the
    // crash supervisor must unload it without respawning.
    let unloaded = wait_until_async(Duration::from_secs(5), || {
        let manager = manager.clone();
        async move { !manager.is_running("clean").await }
    })
    .await;
    assert!(unloaded, "clean exit must remove the plugin");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !manager.is_running("clean").await,
        "clean exit must not be respawned"
    );
}

// ── unload: bye → clean exit, reaped, calls fail fast ──────────────────────

#[tokio::test]
async fn bye_unload_exits_cleanly_and_reaps() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    manager
        .spawn("hello", probe_binary("hello"), None, &[], &[])
        .await
        .expect("spawn hello_plugin");

    let status = manager.unload("hello", &[]).await.expect("unload");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
    assert!(
        !manager.is_running("hello").await,
        "entry removed on unload"
    );
    assert!(
        manager.get("hello").await.is_none(),
        "handle gone after unload"
    );
}

// ── unload: in-flight calls fail cleanly, not hang ─────────────────────────

#[tokio::test]
async fn unload_fails_in_flight_calls_with_plugin_died() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    manager
        .spawn("hung", fixture_script("hung_plugin.sh"), None, &[], &[])
        .await
        .expect("spawn hung fixture");

    // The hung fixture never answers calls, so this one stays in flight
    // until the unload closes the wire.
    let plugin = manager.get("hung").await.expect("registered handle");
    let call_plugin = plugin.clone();
    let call = tokio::spawn(async move {
        call_plugin
            .call("invoke", Some(PLUGIN_NAME), Some(json!({})))
            .await
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let status = manager.unload("hung", &[]).await.expect("unload");
    assert_eq!(status.code(), Some(0), "hung fixture exits 0 on bye");

    let resp = call
        .await
        .expect("call task finished")
        .expect("in-flight call resolves");
    let Msg::Resp { ok, error, .. } = resp else {
        panic!("expected a resp, got {resp:?}");
    };
    assert!(!ok, "in-flight call must fail on unload");
    assert_eq!(
        error.expect("failed resp carries a wire error").kind,
        "PluginDied"
    );
}

// ── respawn: crash-loop detection stops the cycle ──────────────────────────

#[tokio::test]
async fn crash_loop_stops_respawning_after_the_cap() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    manager
        .spawn(
            "crash",
            fixture_script("crash_plugin.sh"),
            Some(test_health()),
            &[],
            &[],
        )
        .await
        .expect("spawn crash fixture");

    // The crash fixture kills itself on the first ping; the health task
    // respawns it until the policy's attempt cap ends the cycle. Transient
    // gaps (backoff between crash and respawn) are shorter than the hold, so
    // the condition only holds once the cycle is truly over.
    let stopped = wait_until_async(Duration::from_secs(5), || {
        let manager = manager.clone();
        async move {
            if manager.is_running("crash").await {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
            !manager.is_running("crash").await
        }
    })
    .await;
    assert!(stopped, "crash loop must stop after the attempt cap");
}

// ── respawn policy: clean exit is NOT respawned ────────────────────────────

#[tokio::test]
async fn clean_exit_is_not_respawned() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    // A generous missed-pong threshold so the fixture's self-exit (not a
    // pong failure) is what ends the health task.
    let health = HealthConfig {
        max_missed_pongs: 10,
        ..test_health()
    };
    manager
        .spawn(
            "clean",
            fixture_script("clean_exit_plugin.sh"),
            Some(health),
            &[],
            &[],
        )
        .await
        .expect("spawn clean-exit fixture");

    // The fixture exits 0 on its own; the health task must unload it
    // without respawning.
    let unloaded = wait_until_async(Duration::from_secs(5), || {
        let manager = manager.clone();
        async move { !manager.is_running("clean").await }
    })
    .await;
    assert!(unloaded, "clean exit must remove the plugin");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !manager.is_running("clean").await,
        "clean exit must not be respawned"
    );
}

// ── respawn identity: a stale respawn leaves a swapped instance alone ──────

#[tokio::test]
async fn stale_respawn_does_not_unload_a_swapped_instance() {
    // A backoff well beyond the swap delay gives the swap time to land while
    // the respawn sleeps (the test-policy cap would truncate the base).
    let policy = RespawnPolicy {
        backoff_base: Duration::from_millis(300),
        backoff_cap: Duration::from_millis(500),
        ..test_policy()
    };
    let manager = Arc::new(PluginManager::new(None, policy));
    manager
        .spawn("hello", probe_binary("hello"), None, &[], &[])
        .await
        .expect("spawn hello_plugin");
    let crashed = manager.get("hello").await.expect("registered handle");

    // The respawn sleeps out the backoff before acting; a swap landing in
    // that window must not be undone by the stale respawn.
    let respawn_task = tokio::spawn({
        let manager = manager.clone();
        async move { manager.respawn("hello", &crashed).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let swapped = manager
        .swap("hello", probe_binary("hello"), &[])
        .await
        .expect("swap during the backoff");
    let outcome = respawn_task
        .await
        .expect("respawn task finished")
        .expect("stale respawn resolves");
    assert_eq!(outcome, RespawnOutcome::Respawned);
    let registered = manager.get("hello").await.expect("registered handle");
    assert!(
        Arc::ptr_eq(&registered, &swapped),
        "the stale respawn must not unload the swapped instance"
    );
    let resp = swapped
        .call("invoke", Some(PLUGIN_NAME), Some(json!({})))
        .await
        .expect("swapped instance still answers");
    assert!(matches!(resp, Msg::Resp { ok: true, .. }));

    manager.unload("hello", &[]).await.expect("teardown");
}

// ── respawn: unknown names fail with not-running ───────────────────────────

#[tokio::test]
async fn respawn_of_an_unknown_plugin_fails_with_not_running() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    // The respawn contract takes the crashed instance (identity check), so
    // any running plugin works as the handle to pass.
    manager
        .spawn("hello", probe_binary("hello"), None, &[], &[])
        .await
        .expect("spawn hello_plugin");
    let probe = manager.get("hello").await.expect("registered handle");

    let err = manager
        .respawn("nope", &probe)
        .await
        .expect_err("unknown name must fail");
    assert!(
        matches!(err, pwr_bot::plugin::PluginError::NotRunning { .. }),
        "unknown name must report NotRunning, got {err:?}"
    );

    manager.unload("hello", &[]).await.expect("teardown");
}

// ── swap: unload old → spawn new → healthy ─────────────────────────────────

#[tokio::test]
async fn swap_replaces_the_running_instance() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    manager
        .spawn(
            "hello",
            probe_binary("hello"),
            Some(test_health()),
            &[],
            &[],
        )
        .await
        .expect("spawn hello_plugin");
    let original = manager.get("hello").await.expect("registered handle");

    let swapped = manager
        .swap("hello", probe_binary("hello"), &[])
        .await
        .expect("swap to the same binary");
    assert!(
        manager.is_running("hello").await,
        "swapped-in instance runs"
    );
    assert!(
        !Arc::ptr_eq(&original, &swapped),
        "swap must yield a fresh handle"
    );

    let resp = swapped
        .call("invoke", Some(PLUGIN_NAME), Some(json!({})))
        .await
        .expect("swapped instance answers");
    assert!(matches!(resp, Msg::Resp { ok: true, .. }));

    manager.unload("hello", &[]).await.expect("teardown");
}

// ── swap: a missing binary fails before touching the running instance ──────

#[tokio::test]
async fn swap_with_a_missing_binary_leaves_the_plugin_running() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    manager
        .spawn("hello", probe_binary("hello"), None, &[], &[])
        .await
        .expect("spawn hello_plugin");
    let original = manager.get("hello").await.expect("registered handle");

    let err = match manager
        .swap("hello", fixture_script("does_not_exist_plugin.sh"), &[])
        .await
    {
        Ok(_) => panic!("swap must fail on a missing binary"),
        Err(err) => err,
    };
    assert!(
        matches!(err, pwr_bot::plugin::PluginError::Spawn { .. }),
        "missing binary must report a spawn error, got {err:?}"
    );
    assert!(
        manager.is_running("hello").await,
        "a failed swap must leave the plugin running"
    );
    let registered = manager.get("hello").await.expect("registered handle");
    assert!(
        Arc::ptr_eq(&registered, &original),
        "a failed swap must keep the original instance"
    );

    manager.unload("hello", &[]).await.expect("teardown");
}

// ── process-group teardown: SIGTERM reaches the whole group on unload ──────

#[cfg(unix)]
#[tokio::test]
async fn unload_signals_the_whole_process_group_with_sigterm() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    let base = group_fixture_base("term");
    let pid_path = base.with_extension("pid");
    let marker_path = base.with_extension("marker");
    let _ = std::fs::remove_file(&pid_path);
    let _ = std::fs::remove_file(&marker_path);

    manager
        .spawn(
            "group",
            fixture_script("group_term_plugin.sh"),
            None,
            &[],
            &[],
        )
        .await
        .expect("spawn group-term fixture");
    let child_pid = wait_for_group_child(&base).await;
    assert!(process_alive(child_pid), "fixture child must be running");

    // The plugin ignores bye/EOF, so unload must SIGTERM the whole group:
    // the plugin dies on SIGTERM (no trap), and the child's marker proves
    // the same group signal reached it.
    let status = manager.unload("group", &[]).await.expect("unload");
    assert_eq!(
        status.code(),
        None,
        "signal death has no exit code: {status}"
    );
    #[cfg(unix)]
    assert_eq!(
        status.signal(),
        Some(15),
        "plugin must die from SIGTERM: {status}"
    );
    let marked = wait_until_async(Duration::from_secs(5), || {
        let marker_path = marker_path.clone();
        async move {
            std::fs::read_to_string(&marker_path)
                .ok()
                .is_some_and(|content| content.contains("term"))
        }
    })
    .await;
    assert!(marked, "group child must receive SIGTERM on unload");
    let dead = wait_until_async(Duration::from_secs(5), || async move {
        !process_alive(child_pid)
    })
    .await;
    assert!(dead, "group child must be dead after unload");
}

#[cfg(unix)]
#[tokio::test]
async fn unload_sigkills_the_group_when_sigterm_is_ignored() {
    let manager = Arc::new(PluginManager::new(None, test_policy()));
    let base = group_fixture_base("kill");
    let pid_path = base.with_extension("pid");
    let _ = std::fs::remove_file(&pid_path);

    manager
        .spawn(
            "group",
            fixture_script("group_kill_plugin.sh"),
            None,
            &[],
            &[],
        )
        .await
        .expect("spawn group-kill fixture");
    let child_pid = wait_for_group_child(&base).await;
    assert!(process_alive(child_pid), "fixture child must be running");

    // The plugin and its child ignore SIGTERM, so unload must escalate to
    // the group SIGKILL after the SIGTERM grace period. The child's death
    // can only come from SIGKILL, which cannot be trapped.
    let status = manager.unload("group", &[]).await.expect("unload");
    assert_eq!(
        status.code(),
        None,
        "signal death has no exit code: {status}"
    );
    #[cfg(unix)]
    assert_eq!(
        status.signal(),
        Some(9),
        "plugin must die from SIGKILL: {status}"
    );
    let dead = wait_until_async(Duration::from_secs(5), || async move {
        !process_alive(child_pid)
    })
    .await;
    assert!(
        dead,
        "group child must be SIGKILLed after the SIGTERM grace"
    );
}

// ── tasks: manifest task loops invoke the declared command ─────────────────

#[tokio::test]
async fn task_loop_invokes_the_declared_command_on_interval() {
    let bus = Arc::new(EventBus::new());
    let seen = Arc::new(tokio::sync::Mutex::new(Vec::<PluginEvent>::new()));
    bus.register_callback({
        let seen = seen.clone();
        move |event: PluginEvent| {
            let seen = seen.clone();
            async move {
                seen.lock().await.push(event);
                Ok(())
            }
        }
    });
    let manager = Arc::new(PluginManager::new(None, test_policy()).with_event_bus(bus));
    manager
        .spawn(
            "hello",
            probe_binary("hello"),
            None,
            &[],
            &[TaskDef {
                name: "tick".into(),
                interval_secs: 1,
                command: "hello.tick".into(),
            }],
        )
        .await
        .expect("spawn hello_plugin");

    // The task must fire repeatedly, not once: wait for two ticks.
    let ticked = wait_until_async(Duration::from_secs(5), || {
        let seen = seen.clone();
        async move {
            seen.lock()
                .await
                .iter()
                .filter(|e| e.name == "hello.tick")
                .count()
                >= 2
        }
    })
    .await;
    assert!(ticked, "the task loop must invoke its command repeatedly");

    // Unload must cancel the loop. Two samples separated by more than the 1s
    // interval prove the count is stable, not just slow.
    manager.unload("hello", &[]).await.expect("teardown");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let settled = seen
        .lock()
        .await
        .iter()
        .filter(|e| e.name == "hello.tick")
        .count();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let later = seen
        .lock()
        .await
        .iter()
        .filter(|e| e.name == "hello.tick")
        .count();
    assert_eq!(later, settled, "no ticks may arrive after unload");
}
