//! Plugin lifecycle: a manager that owns running plugin subprocesses.
//!
//! The [`PluginManager`] keys [`RunningPlugin`] handles by plugin name and
//! provides the lifecycle beyond spawn/call/stop: periodic health checks
//! (ping/pong with a missed-pong threshold), graceful unload, crash respawn
//! with exponential backoff, and binary swap. The framework merge —
//! assembling the plugin command `Vec` before `Framework::builder().build()`
//! — stays a documented seam in [`crate::plugin::command`] for a later
//! ticket (#113).
//!
//! Lifecycle policy:
//! - **Supervision**: every spawned instance gets a crash-supervisor task
//!   awaiting its exit status, regardless of any health config: exit 0 is a
//!   clean, intentional exit (unloaded without respawn); a crash (nonzero
//!   exit or signal) is respawned under the backoff/crash-loop policy.
//! - **Health**: when a [`HealthConfig`] is supplied, a per-plugin
//!   background task sends `ping` every [`HealthConfig::interval`]. A plugin
//!   that misses [`HealthConfig::max_missed_pongs`] consecutive pongs is
//!   unloaded and respawned. The health task owns liveness only — exit
//!   classification belongs to the crash supervisor.
//! - **Unload**: removes the handle so new calls fail fast, best-effort
//!   unregisters the plugin's guild commands (the #112 seam), then delegates
//!   to [`RunningPlugin::stop`] (`bye` → EOF → grace → SIGTERM to the whole
//!   process group → grace → SIGKILL to the group), whose reaper task owns
//!   the final `wait()` (no zombies). Plugins spawn with `process_group(0)`,
//!   so the group signals reach any descendants.
//! - **Respawn**: inline exponential backoff with deterministic jitter (no
//!   `backon` dependency), capped attempts, and a crash-loop window: too
//!   many respawns within the window stop the cycle and leave the plugin
//!   down.
//! - **Swap**: unload the old binary, spawn the new one. External install
//!   (resolving a pin to a binary path) is #110; the manager takes the new
//!   path as input.
//! - **Tasks**: manifest `tasks[]` drive a per-task loop that invokes the
//!   plugin's command on its interval; loops end on unload (the stop flag)
//!   or when the plugin is reaped (the respawn restarts them).

use std::collections::HashMap;
use std::collections::VecDeque;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use log::debug;
use log::info;
use log::warn;
use poise::serenity_prelude as serenity;
use pwr_plugin_protocol::TaskDef;
use tokio::sync::Mutex;

use crate::event::event_bus::EventBus;
use crate::plugin::HostServices;
use crate::plugin::PluginError;
use crate::plugin::PluginEventRouter;
use crate::plugin::RunningPlugin;
use crate::plugin::command::register_in_guild;
use crate::plugin::interaction::DEFAULT_VIEW_TIMEOUT;
use crate::plugin::modal::ModalRouter;

/// Per-plugin health-check configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthConfig {
    /// Interval between liveness pings.
    pub interval: Duration,
    /// Consecutive missed pongs before the plugin is considered dead.
    pub max_missed_pongs: u32,
    /// How long to wait for a `pong` after each `ping` before counting it
    /// missed.
    pub pong_window: Duration,
}

impl Default for HealthConfig {
    /// Production defaults: a 30s ping interval, 3 missed pongs, a 5s pong
    /// window (~90s to declare a plugin dead). Tests pass short intervals.
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            max_missed_pongs: 3,
            pong_window: Duration::from_secs(5),
        }
    }
}

/// Respawn policy: backoff bounds, attempt cap, and the crash-loop window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RespawnPolicy {
    /// Backoff base interval; doubled per attempt.
    pub backoff_base: Duration,
    /// Upper bound on a single backoff delay.
    pub backoff_cap: Duration,
    /// Max respawns within [`RespawnPolicy::crash_loop_window`] before the
    /// cycle stops.
    pub max_attempts: u32,
    /// Rolling window over which [`RespawnPolicy::max_attempts`] applies.
    pub crash_loop_window: Duration,
}

impl Default for RespawnPolicy {
    /// Production defaults: 500ms base, 30s cap, 5 attempts within 60s.
    fn default() -> Self {
        Self {
            backoff_base: Duration::from_millis(500),
            backoff_cap: Duration::from_secs(30),
            max_attempts: 5,
            crash_loop_window: Duration::from_secs(60),
        }
    }
}

/// The outcome of a [`PluginManager::respawn`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RespawnOutcome {
    /// A fresh instance was spawned.
    Respawned,
    /// The crash-loop cap was hit; the plugin stays down.
    CrashLoop,
}

/// Exponential backoff with jitter for one respawn attempt:
/// `min(cap, base * 2^attempt)` scaled by `1 - jitter`, where `jitter` is in
/// `[0, 1)`. Returns a whole number of milliseconds; the delay is at most
/// `cap`.
pub(crate) fn backoff_delay(base: Duration, cap: Duration, attempt: u32, jitter: f64) -> Duration {
    let exponential = base.saturating_mul(2_u32.saturating_pow(attempt));
    let bounded = exponential.min(cap);
    let factor = 1.0 - jitter.clamp(0.0, 1.0);
    Duration::from_millis((bounded.as_millis() as f64 * factor) as u64)
}

/// Deterministic pseudo-jitter for the `attempt`-th backoff, in `[0, 1)`.
/// Fractional parts of golden-ratio multiples are well distributed, so the
/// sequence needs no RNG dependency.
fn jitter_for(attempt: u32) -> f64 {
    (attempt as f64 * 0.618_033_988_749_894_8).fract()
}

/// Rolling-window respawn tracker for one plugin: records when respawns
/// happened and decides whether the crash-loop cap is hit.
#[derive(Debug)]
pub(crate) struct CrashLoopGuard {
    /// Respawn timestamps still within the window.
    attempts: VecDeque<Instant>,
    /// Max respawns within `window`.
    max: u32,
    /// The rolling window.
    window: Duration,
}

impl CrashLoopGuard {
    /// A guard allowing up to `max` respawns within `window`.
    pub fn new(max: u32, window: Duration) -> Self {
        Self {
            attempts: VecDeque::new(),
            max,
            window,
        }
    }

    /// Records a respawn at `now`, first pruning attempts older than the
    /// window.
    pub fn record(&mut self, now: Instant) {
        self.prune(now);
        self.attempts.push_back(now);
    }

    /// Whether the cap is hit at `now`: `max` or more attempts within the
    /// window.
    pub fn is_limited(&self, now: Instant) -> bool {
        self.attempts_in_window(now) >= self.max
    }

    /// How many recorded attempts are within the window at `now`.
    pub fn attempts_in_window(&self, now: Instant) -> u32 {
        let count = match now.checked_sub(self.window) {
            Some(cutoff) => self.attempts.iter().filter(|t| **t >= cutoff).count(),
            None => self.attempts.len(),
        };
        count as u32
    }

    fn prune(&mut self, now: Instant) {
        let Some(cutoff) = now.checked_sub(self.window) else {
            return;
        };
        while self.attempts.front().is_some_and(|t| *t < cutoff) {
            self.attempts.pop_front();
        }
    }
}

/// One registered plugin: the running handle, its spawn spec (binary path),
/// health config, the stop flag for its health task, and the Discord events
/// it subscribed to at spawn.
struct Entry {
    /// The running subprocess handle.
    plugin: Arc<RunningPlugin>,
    /// Binary path used to respawn after a crash.
    path: PathBuf,
    /// Health config; `None` disables the health task.
    health: Option<HealthConfig>,
    /// Set when the entry is unloaded; the health task checks it each loop,
    /// and the crash supervisor consumes it to suppress respawns during
    /// teardown.
    stop: Arc<AtomicBool>,
    /// Discord events the plugin declared in its manifest; re-applied on
    /// respawn and swap (subscriptions are keyed by name, so re-subscribing
    /// is idempotent).
    event_handlers: Vec<String>,
    /// Manifest `tasks[]`: per-task loops invoke the declared command on its
    /// interval; re-applied on respawn and swap.
    tasks: Vec<TaskDef>,
}

/// Owns running plugins by name and drives the lifecycle: health checks,
/// unload, respawn, and swap.
///
/// Methods that start background tasks (`spawn`, `respawn`, `swap` — health
/// checks, task loops, and crash supervision) take `self: &Arc<Self>` because
/// the spawned task holds an `Arc` clone of the manager; the rest take
/// `&self`.
pub struct PluginManager {
    /// Running plugins keyed by plugin name.
    plugins: Mutex<HashMap<String, Entry>>,
    /// Crash-loop tracking per plugin name.
    crash_loops: Mutex<HashMap<String, CrashLoopGuard>>,
    /// Discord HTTP client for guild-command cleanup on unload/swap; `None`
    /// skips it (e.g. in tests).
    http: Option<Arc<serenity::Http>>,
    /// Host services (Discord I/O seam, config subset, KV store) handed to
    /// every spawned plugin; `None` spawns without services (e.g. in tests).
    services: Option<Arc<HostServices>>,
    /// Event bus plugin→host events are broadcast on; `None` drops them
    /// (logged) instead.
    event_bus: Option<Arc<EventBus>>,
    /// Router Discord events are fanned out through; a plugin with
    /// `event_handlers` is subscribed to them at spawn.
    event_router: Option<Arc<PluginEventRouter>>,
    /// Respawn backoff/crash-loop policy.
    respawn_policy: RespawnPolicy,
    /// Author-keyed routes for plugin-opened modals: a `host.open_modal`
    /// binds the author to the calling session, and the author's later
    /// submission rides the route back to that session.
    pub(crate) modals: ModalRouter,
}

impl PluginManager {
    /// A manager with the given optional Discord HTTP client (for guild
    /// command cleanup) and respawn policy.
    pub fn new(http: Option<Arc<serenity::Http>>, respawn_policy: RespawnPolicy) -> Self {
        Self {
            plugins: Mutex::new(HashMap::new()),
            crash_loops: Mutex::new(HashMap::new()),
            http,
            services: None,
            event_bus: None,
            event_router: None,
            respawn_policy,
            modals: ModalRouter::new(DEFAULT_VIEW_TIMEOUT),
        }
    }

    /// Wires host services into the manager so every spawned plugin can be
    /// served `host.*` calls. Builder-style: consumes `self`.
    pub fn with_host_services(mut self, services: Arc<HostServices>) -> Self {
        self.services = Some(services);
        self
    }

    /// Wires an event bus into the manager so every spawned plugin's
    /// plugin→host events are broadcast on it. Builder-style: consumes
    /// `self`.
    pub fn with_event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Wires an event router into the manager so every spawned plugin with
    /// manifest `event_handlers` is subscribed to those Discord events.
    /// Builder-style: consumes `self`.
    pub fn with_event_router(mut self, event_router: Arc<PluginEventRouter>) -> Self {
        self.event_router = Some(event_router);
        self
    }

    /// Spawns the binary at `path` under the name `name`, starts its
    /// unconditional crash-supervisor task, starts its health task if
    /// `health` is given, subscribes it to `event_handlers`
    /// (the manifest's Discord events) when an event router is wired, and
    /// starts one task loop per manifest `tasks[]` entry (each invokes the
    /// declared command on its interval). Fails with
    /// [`PluginError::AlreadyRunning`] if the name is already
    /// registered.
    pub async fn spawn(
        self: &Arc<Self>,
        name: &str,
        path: impl AsRef<Path>,
        health: Option<HealthConfig>,
        event_handlers: &[String],
        tasks: &[TaskDef],
    ) -> Result<Arc<RunningPlugin>, PluginError> {
        let path = path.as_ref().to_path_buf();
        {
            let plugins = self.plugins.lock().await;
            if plugins.contains_key(name) {
                return Err(PluginError::AlreadyRunning {
                    name: name.to_string(),
                });
            }
        }
        // Spawns with the manager itself wired in so `host.open_view` on any
        // plugin's reader can resolve siblings as targets.
        let plugin = Arc::new(
            RunningPlugin::spawn_with(
                &path,
                self.services.clone(),
                Some(Arc::clone(self)),
                self.event_bus.clone(),
            )
            .await?,
        );
        let stop = Arc::new(AtomicBool::new(false));
        let entry = Entry {
            plugin: plugin.clone(),
            path: path.clone(),
            health: health.clone(),
            stop: stop.clone(),
            event_handlers: event_handlers.to_vec(),
            tasks: tasks.to_vec(),
        };
        // Register under the name. A concurrent spawn that won the race
        // reports itself here: the guard drops before the stop, so the
        // plugins map is never blocked across the wait, and the winner's
        // entry is left untouched.
        let registered = {
            let mut plugins = self.plugins.lock().await;
            match plugins.entry(name.to_string()) {
                std::collections::hash_map::Entry::Occupied(_) => false,
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(entry);
                    true
                }
            }
        };
        if !registered {
            plugin.stop().await?;
            return Err(PluginError::AlreadyRunning {
                name: name.to_string(),
            });
        }
        if let Some(router) = &self.event_router {
            router.subscribe(name, event_handlers).await;
        }
        start_crash_supervisor(self, name.to_string(), plugin.clone(), stop.clone());
        if let Some(config) = health {
            start_health_task(self, name.to_string(), config, stop.clone());
        }
        for task in tasks {
            start_task_loop(self, name.to_string(), task.clone(), stop.clone());
        }
        Ok(plugin)
    }

    /// The running handle for `name`, if registered.
    pub async fn get(&self, name: &str) -> Option<Arc<RunningPlugin>> {
        self.plugins
            .lock()
            .await
            .get(name)
            .map(|entry| entry.plugin.clone())
    }

    /// Whether a plugin named `name` is currently registered and running.
    pub async fn is_running(&self, name: &str) -> bool {
        self.plugins.lock().await.contains_key(name)
    }

    /// The names of all currently registered plugins, sorted.
    pub async fn running_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.plugins.lock().await.keys().cloned().collect();
        names.sort();
        names
    }

    /// Unloads `name`: removes the handle (new calls fail fast with
    /// [`PluginError::NotRunning`]), best-effort unregisters the plugin's
    /// guild commands in `guild_ids` (an empty command slice unregisters;
    /// the guild list comes from the guild_plugins table, the #112 seam),
    /// then gracefully stops the subprocess via [`RunningPlugin::stop`]
    /// (`bye` → EOF → grace → SIGTERM to the whole process group → grace →
    /// SIGKILL to the group) and returns its final exit status. The
    /// crash-loop guard for `name` is cleared: a manual unload ends any
    /// crash cycle, so a later spawn starts fresh.
    ///
    /// The reaper task owns the final `wait()`, so no zombie is left behind;
    /// in-flight calls fail with a `PluginDied` wire error when stdout
    /// closes. Plugins spawn with `process_group(0)`, so the group signals
    /// reach any descendants the plugin spawned.
    pub async fn unload(
        &self,
        name: &str,
        guild_ids: &[serenity::GuildId],
    ) -> Result<ExitStatus, PluginError> {
        match self.unload_matching(name, guild_ids, None).await {
            Some(result) => {
                self.crash_loops.lock().await.remove(name);
                result
            }
            None => Err(PluginError::NotRunning {
                name: name.to_string(),
            }),
        }
    }

    /// Like [`PluginManager::unload`], but only removes `name` when its
    /// registered handle is `expected` (identity check; `None` matches any
    /// handle). Returns `None` when nothing matched — the name is absent or
    /// a different instance now owns it — and `Some(..)` with the stop
    /// result otherwise. Does not clear the crash-loop guard: the respawn
    /// path relies on the guard surviving across attempts.
    async fn unload_matching(
        &self,
        name: &str,
        guild_ids: &[serenity::GuildId],
        expected: Option<&Arc<RunningPlugin>>,
    ) -> Option<Result<ExitStatus, PluginError>> {
        let entry = {
            let mut plugins = self.plugins.lock().await;
            match plugins.get(name) {
                Some(entry) if expected.is_none_or(|e| Arc::ptr_eq(&entry.plugin, e)) => {
                    plugins.remove(name)
                }
                _ => None,
            }
        };
        let entry = entry?;
        // The session is dying: a later author submission must not route to
        // a process that never opened the modal (a respawned plugin has no
        // in-memory correlation state). A respawned plugin rebinds on its
        // next `host.open_modal`.
        self.drop_modal_routes(name).await;
        entry.stop.store(true, Ordering::Relaxed);
        if let Some(http) = &self.http {
            for guild_id in guild_ids {
                if let Err(e) = register_in_guild(http, &[], *guild_id).await {
                    warn!("failed to unregister plugin `{name}` commands in guild {guild_id}: {e}");
                }
            }
        }
        Some(entry.plugin.stop().await)
    }

    /// Respawns `name` after a crash of the instance `crashed`: reads the
    /// stored spawn spec, waits out the exponential backoff for the current
    /// attempt, then unloads `crashed` and spawns a fresh one. The unload is
    /// gated on `crashed` still being the registered handle: a swap that
    /// landed during the backoff — or a concurrent unload — makes the
    /// respawn a no-op that reports [`RespawnOutcome::Respawned`] and leaves
    /// the current instance alone. Returns [`RespawnOutcome::CrashLoop`]
    /// instead of spawning when the cap is hit, leaving the plugin unloaded
    /// (down) — the crashed instance has already been reaped, so the map
    /// entry is removed with no further work. [`PluginError::NotRunning`] is
    /// reserved for a name that was never registered.
    ///
    /// Callers pass the crashed handle so the respawn never unloads a
    /// differently-registered instance; the health task and crash supervisor
    /// each pass the instance they were watching.
    pub async fn respawn(
        self: &Arc<Self>,
        name: &str,
        crashed: &Arc<RunningPlugin>,
    ) -> Result<RespawnOutcome, PluginError> {
        let (limited, attempt) = self
            .with_crash_loop_guard(name, |guard| {
                let now = Instant::now();
                (guard.is_limited(now), guard.attempts_in_window(now))
            })
            .await;
        if limited {
            // The cap is hit: the crashed instance is dead and must leave
            // the map so the plugin is truly down. The stop is cheap — the
            // exit status is already published. The guard is cleared too, so
            // a later spawn starts a fresh cycle.
            let _ = self.unload_matching(name, &[], Some(crashed)).await;
            self.crash_loops.lock().await.remove(name);
            return Ok(RespawnOutcome::CrashLoop);
        }
        // Read the spawn spec before the backoff: `crashed` came from the
        // map moments ago, so the entry exists now. A swap during the sleep
        // replaces the instance, and the identity-gated unload below then
        // leaves the fresh one alone. `NotRunning` is reserved for a name
        // that was never registered.
        let (path, health, event_handlers, tasks) = {
            let plugins = self.plugins.lock().await;
            match plugins.get(name) {
                None => {
                    return Err(PluginError::NotRunning {
                        name: name.to_string(),
                    });
                }
                Some(entry) => (
                    entry.path.clone(),
                    entry.health.clone(),
                    entry.event_handlers.clone(),
                    entry.tasks.clone(),
                ),
            }
        };
        let delay = backoff_delay(
            self.respawn_policy.backoff_base,
            self.respawn_policy.backoff_cap,
            attempt,
            jitter_for(attempt),
        );
        tokio::time::sleep(delay).await;

        match self.unload_matching(name, &[], Some(crashed)).await {
            // The crashed instance is no longer registered — a swap won or
            // an unload landed during the backoff — so there is nothing to
            // do and the current instance is left untouched.
            None => return Ok(RespawnOutcome::Respawned),
            Some(result) => result?,
        };
        self.with_crash_loop_guard(name, |guard| guard.record(Instant::now()))
            .await;
        self.spawn(name, &path, health, &event_handlers, &tasks)
            .await?;
        Ok(RespawnOutcome::Respawned)
    }

    /// Swaps `name` to a new binary: validates that the new path exists,
    /// unloads the old instance (deleting stale guild commands in
    /// `guild_ids`), then spawns the new one with the same health config. A
    /// missing binary fails the swap before the running instance is touched.
    /// The new binary path is the input; resolving a pin to a path is the
    /// #110 install seam.
    pub async fn swap(
        self: &Arc<Self>,
        name: &str,
        new_path: impl AsRef<Path>,
        guild_ids: &[serenity::GuildId],
    ) -> Result<Arc<RunningPlugin>, PluginError> {
        let new_path = new_path.as_ref().to_path_buf();
        if !new_path.exists() {
            return Err(PluginError::Spawn {
                path: new_path,
                source: io::Error::from(io::ErrorKind::NotFound),
            });
        }
        let (health, event_handlers, tasks) = {
            let plugins = self.plugins.lock().await;
            match plugins.get(name) {
                Some(entry) => (
                    entry.health.clone(),
                    entry.event_handlers.clone(),
                    entry.tasks.clone(),
                ),
                None => (None, Vec::new(), Vec::new()),
            }
        };
        self.unload(name, guild_ids).await?;
        self.spawn(name, &new_path, health, &event_handlers, &tasks)
            .await
    }

    /// Runs `f` with the crash-loop guard for `name` (creating it on first
    /// use) and returns its value, releasing the guard lock afterwards.
    async fn with_crash_loop_guard<T>(
        &self,
        name: &str,
        f: impl FnOnce(&mut CrashLoopGuard) -> T,
    ) -> T {
        let mut loops = self.crash_loops.lock().await;
        let guard = loops.entry(name.to_string()).or_insert_with(|| {
            CrashLoopGuard::new(
                self.respawn_policy.max_attempts,
                self.respawn_policy.crash_loop_window,
            )
        });
        f(guard)
    }
}

/// Spawns one crash-supervisor task for a plugin instance, mirroring
/// [`start_health_task`]: a plain (non-async) function so the spawned
/// task's future awaits `respawn` → `spawn`, which would make the `Send`
/// obligation cycle through `spawn`.
fn start_crash_supervisor(
    manager: &Arc<PluginManager>,
    name: String,
    plugin: Arc<RunningPlugin>,
    stop: Arc<AtomicBool>,
) {
    tokio::spawn(crash_supervisor(Arc::clone(manager), name, plugin, stop));
}

/// One crash-supervisor task for a single plugin instance. Awaits the
/// instance's exit watch channel and owns every exit-driven decision: a
/// clean exit (code 0) unloads without respawn, anything else is a crash
/// and gets respawned under the policy. Runs unconditionally, so plugins
/// spawned without a [`HealthConfig`] — the core plugins — are supervised
/// too; `HealthConfig` stays solely responsible for liveness pings.
///
/// Ownership: this task — never [`health_loop`] — classifies an exit, so a
/// death cannot be handled twice. An intentional teardown raises the
/// instance's stop flag in [`PluginManager::unload_matching`] *before*
/// stopping the process, so any exit observed after it (including a
/// SIGKILL escalation) returns without respawning. A respawned instance's
/// spawn starts its own supervisor.
async fn crash_supervisor(
    manager: Arc<PluginManager>,
    name: String,
    plugin: Arc<RunningPlugin>,
    stop: Arc<AtomicBool>,
) {
    let Ok(status) = plugin.wait_for_exit().await else {
        // The reaper ended without publishing a status. Nothing observes
        // this process anymore; treat the unknown-status death like a crash.
        if !stop.load(Ordering::Relaxed) {
            warn!("plugin {name} died with no exit status; respawning");
            respawn_or_warn(&manager, &name, &plugin).await;
        }
        return;
    };
    // Teardown owns this death: the flag goes up before the process stops,
    // so an unload/swap landing first must not be undone here.
    if stop.load(Ordering::Relaxed) {
        return;
    }
    if status.code() == Some(0) {
        info!("plugin {name} exited cleanly; unloading without respawn");
        match manager.unload_matching(&name, &[], Some(&plugin)).await {
            // A swap won; the fresh instance stays.
            None => {}
            Some(result) => {
                manager.crash_loops.lock().await.remove(&name);
                if let Err(e) = result {
                    warn!("failed to unload clean-exited plugin {name}: {e}");
                }
            }
        }
        return;
    }
    warn!("plugin {name} crashed: {status}; respawning");
    respawn_or_warn(&manager, &name, &plugin).await;
}

/// Spawns one health-check task for a plugin instance. A plain (non-async)
/// function: the spawned task's future awaits `respawn` → `spawn`, which
/// itself starts another health task, so spawning the task inline would make
/// the `Send` obligation cycle (`spawn`'s future holds `health_loop`'s
/// future, which awaits `spawn`'s future). Function boundaries do not
/// propagate that obligation, breaking the cycle.
fn start_health_task(
    manager: &Arc<PluginManager>,
    name: String,
    config: HealthConfig,
    stop: Arc<AtomicBool>,
) {
    tokio::spawn(health_loop(Arc::clone(manager), name, config, stop));
}

/// One health-check task for a single plugin instance. Each loop iteration
/// checks the stop flag, then whether the fetched instance was already
/// reaped (the crash supervisor owns that respawn, so the task just ends),
/// then pings and counts missed pongs. A pong that lands late — after its
/// window but before the next ping — resets the missed count instead of
/// being double-counted; this is deliberate leniency for a busy plugin. On
/// the missed-pong threshold it unloads and respawns; the fresh instance
/// gets its own health task.
async fn health_loop(
    manager: Arc<PluginManager>,
    name: String,
    config: HealthConfig,
    stop: Arc<AtomicBool>,
) {
    let mut missed: u32 = 0;
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let Some(plugin) = manager.get(&name).await else {
            return; // unloaded concurrently
        };
        // A swap or unload may have landed between the fetch and the checks
        // below; honour a concurrent stop flag.
        if stop.load(Ordering::Relaxed) {
            return;
        }
        // A reaped plugin ends this task: the crash supervisor owns the
        // exit-driven decisions, and the fresh instance's spawn starts its
        // own health task.
        if plugin.exit_status().is_some() {
            return;
        }
        // Liveness: a pong must arrive within the window or the ping counts
        // as missed.
        let before = plugin.pongs_received();
        if let Err(e) = plugin.ping().await {
            warn!("plugin {name} ping failed: {e}");
            missed += 1;
        } else {
            tokio::time::sleep(config.pong_window).await;
            if plugin.pongs_received() == before {
                missed += 1;
            } else {
                missed = 0;
            }
        }
        if missed >= config.max_missed_pongs {
            warn!("plugin {name} missed {missed} pongs; unloading and respawning");
            respawn_or_warn(&manager, &name, &plugin).await;
            return;
        }
        tokio::time::sleep(config.interval).await;
    }
}

/// Spawns one task loop for a plugin instance, mirroring
/// [`start_health_task`]: a plain (non-async) function so the spawned task's
/// `Send` obligation does not cycle through `spawn`.
fn start_task_loop(
    manager: &Arc<PluginManager>,
    name: String,
    task: TaskDef,
    stop: Arc<AtomicBool>,
) {
    tokio::spawn(task_loop(Arc::clone(manager), name, task, stop));
}

/// One task loop for a single plugin instance. Each loop iteration checks
/// the stop flag, then the plugin's exit status (a reaped plugin ends this
/// task; the crash supervisor owns exit-path respawn and the fresh
/// instance's spawn starts its own task loops), then invokes the task's
/// command and sleeps the interval. The interval is clamped to at least 1s
/// so a 0s interval cannot busy-spin the plugin.
async fn task_loop(
    manager: Arc<PluginManager>,
    name: String,
    task: TaskDef,
    stop: Arc<AtomicBool>,
) {
    info!(
        "starting task {} on plugin {name}: every {}s, command {}",
        task.name, task.interval_secs, task.command
    );
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let Some(plugin) = manager.get(&name).await else {
            return; // unloaded concurrently
        };
        if stop.load(Ordering::Relaxed) {
            return;
        }
        // A reaped plugin ends this loop; the crash supervisor owns
        // exit-path respawn and the fresh instance's spawn starts its own
        // task loops.
        if plugin.exit_status().is_some() {
            return;
        }
        match plugin.call("invoke", Some(&task.command), None).await {
            Ok(_) => debug!("task {} on plugin {name} ran `{}`", task.name, task.command),
            Err(e) => warn!("task {} on plugin {name} failed: {e}", task.name),
        }
        tokio::time::sleep(Duration::from_secs(task.interval_secs.max(1))).await;
    }
}

/// Best-effort respawn used by the crash supervisor and health task: logs
/// the outcome instead of returning it, since the task has no caller to
/// report to.
async fn respawn_or_warn(manager: &Arc<PluginManager>, name: &str, crashed: &Arc<RunningPlugin>) {
    match manager.respawn(name, crashed).await {
        Ok(RespawnOutcome::Respawned) => {}
        Ok(RespawnOutcome::CrashLoop) => {
            warn!("plugin {name} hit the crash-loop cap; stays down");
        }
        Err(e) => warn!("failed to respawn plugin {name}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_policy() -> RespawnPolicy {
        RespawnPolicy {
            backoff_base: Duration::from_millis(1),
            backoff_cap: Duration::from_millis(2),
            max_attempts: 3,
            crash_loop_window: Duration::from_secs(1),
        }
    }

    // ── backoff ──────────────────────────────────────────────────────────────

    #[test]
    fn backoff_doubles_per_attempt_up_to_the_cap() {
        let base = Duration::from_millis(100);
        let cap = Duration::from_millis(800);
        assert_eq!(backoff_delay(base, cap, 0, 0.0), Duration::from_millis(100));
        assert_eq!(backoff_delay(base, cap, 1, 0.0), Duration::from_millis(200));
        assert_eq!(backoff_delay(base, cap, 2, 0.0), Duration::from_millis(400));
        assert_eq!(backoff_delay(base, cap, 3, 0.0), Duration::from_millis(800));
        assert_eq!(backoff_delay(base, cap, 4, 0.0), Duration::from_millis(800));
        assert_eq!(
            backoff_delay(base, cap, 10, 0.0),
            Duration::from_millis(800)
        );
    }

    #[test]
    fn backoff_jitter_scales_the_delay_down() {
        let base = Duration::from_millis(1000);
        let cap = Duration::from_secs(10);
        let full = backoff_delay(base, cap, 0, 0.0);
        let jittered = backoff_delay(base, cap, 0, 0.5);
        assert_eq!(jittered, Duration::from_millis(500));
        assert!(jittered < full);
        assert!(jittered > Duration::ZERO);
    }

    #[test]
    fn jitter_is_bounded_and_deterministic() {
        for attempt in 0..32 {
            let jitter = jitter_for(attempt);
            assert!(
                (0.0..1.0).contains(&jitter),
                "jitter out of range: {jitter}"
            );
        }
        assert_eq!(jitter_for(3), jitter_for(3));
    }

    // ── crash-loop guard ─────────────────────────────────────────────────────

    #[test]
    fn crash_loop_guard_limits_respawns_within_the_window() {
        let base = Instant::now();
        let mut guard = CrashLoopGuard::new(3, Duration::from_secs(60));
        assert!(!guard.is_limited(base));
        guard.record(base + Duration::from_secs(1));
        guard.record(base + Duration::from_secs(2));
        guard.record(base + Duration::from_secs(3));
        assert!(guard.is_limited(base + Duration::from_secs(4)));
        assert_eq!(guard.attempts_in_window(base + Duration::from_secs(4)), 3);
    }

    #[test]
    fn crash_loop_guard_forgets_attempts_older_than_the_window() {
        let base = Instant::now();
        let mut guard = CrashLoopGuard::new(2, Duration::from_secs(10));
        guard.record(base);
        guard.record(base + Duration::from_secs(1));
        assert!(guard.is_limited(base + Duration::from_secs(2)));

        // The window slides: the first attempt ages out, so the cap no
        // longer holds and a fresh attempt is allowed.
        let later = base + Duration::from_secs(11);
        assert!(!guard.is_limited(later));
        assert_eq!(guard.attempts_in_window(later), 1);
    }

    // ── manager map ops (no subprocesses involved) ───────────────────────────

    #[tokio::test]
    async fn is_running_is_false_for_an_unknown_plugin() {
        let manager = Arc::new(PluginManager::new(None, test_policy()));
        assert!(!manager.is_running("nope").await);
        assert!(manager.get("nope").await.is_none());
    }

    #[tokio::test]
    async fn unload_of_an_unknown_plugin_fails_with_not_running() {
        let manager = Arc::new(PluginManager::new(None, test_policy()));
        let err = manager.unload("nope", &[]).await.unwrap_err();
        assert!(matches!(err, PluginError::NotRunning { .. }));
    }

    #[tokio::test]
    async fn spawn_subscribes_the_plugin_to_its_event_handlers() {
        let router = Arc::new(PluginEventRouter::new());
        let manager =
            Arc::new(PluginManager::new(None, test_policy()).with_event_router(router.clone()));
        let stub =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stubborn_plugin.sh");
        manager
            .spawn("stubborn", stub, None, &["voice_state".to_string()], &[])
            .await
            .expect("spawn stubborn fixture");
        assert_eq!(router.subscribers("voice_state").await, ["stubborn"]);
        manager.unload("stubborn", &[]).await.expect("teardown");
    }
}
