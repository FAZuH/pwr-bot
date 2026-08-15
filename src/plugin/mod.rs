//! Plugin runtime: spawns plugin subprocesses and speaks the wire protocol
//! over JSON-Lines stdio.
//!
//! A plugin is a standalone executable exchanging [`Msg`] values with the
//! host: one compact JSON object per line on stdout, terminated by `\n`.
//! stdout carries only protocol lines; stderr is the free logging channel and
//! is forwarded into the host's `log` output.
//!
//! Spawn flow: the plugin announces `hello` first, the host validates it
//! (version plus `host.*` caps, rejecting before any work) and answers with
//! its own `hello` as the ack. Calls then flow host→plugin, correlated by
//! monotonic ids; each `resp` is matched to its waiting call through a
//! oneshot channel. Events flow host→plugin as one-way pushes; the
//! interaction engine pushes `view.timeout` when a view session is
//! abandoned. When the plugin's stdout closes or the wire corrupts,
//! every in-flight call fails with a `PluginDied` wire error and the reaper
//! task reaps the child via `wait()`.
//!
//! Lifecycle beyond spawn/call/stop lives in [`manager`]: health checks,
//! unload, crash respawn, and binary swap over a map of [`RunningPlugin`]
//! handles. External install from a pinned catalog lives in [`install`];
//! KV and per-guild sets are later work (#112). Dropping a
//! [`RunningPlugin`] kills its subprocess via the `Drop` impl, so unloading
//! a plugin is drop-and-forget; graceful unload is [`RunningPlugin::stop`].

pub mod command;
pub mod error;
pub mod host;
pub mod install;
pub mod interaction;
pub mod manager;

use std::collections::HashMap;
use std::path::Path;
use std::process::ExitStatus;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub use error::InstallError;
pub use error::PluginError;
pub use host::HostConfig;
pub use host::HostError;
pub use host::HostIo;
pub use host::HostServices;
pub use host::KvError;
pub use host::KvStore;
pub use host::PgKvStore;
pub use host::SerenityHostIo;
pub use install::CatalogEntry;
pub use install::PluginCatalog;
pub use interaction::InteractionEngine;
pub use interaction::InteractionError;
use log::debug;
use log::info;
use log::warn;
pub use manager::HealthConfig;
pub use manager::PluginManager;
pub use manager::RespawnOutcome;
pub use manager::RespawnPolicy;
use pwr_plugin_protocol::ALL_CAPS;
use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::CallIdSeq;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::WireError;
use pwr_plugin_protocol::validate_caps;
use serde_json::Value;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::ChildStderr;
use tokio::process::ChildStdin;
use tokio::process::ChildStdout;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio::sync::watch;

/// How long the host waits for the plugin's `hello` after spawn.
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a [`RunningPlugin::call`] waits for its correlated `resp`.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// How long [`RunningPlugin::stop`] waits for the plugin to exit after `bye`
/// before killing it.
const STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// A running plugin subprocess: owns the stdio pipes, the reader/waiter
/// tasks, and call correlation.
pub struct RunningPlugin {
    /// Plugin identity announced in its hello, e.g. `hello`.
    name: String,
    /// Writer to the plugin's stdin. `None` once stopped: dropping the handle
    /// closes the pipe, and the plugin treats stdin EOF as exit.
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    /// In-flight calls: correlation id -> the oneshot awaiting its resp.
    inflight: Arc<Mutex<HashMap<u64, oneshot::Sender<Msg>>>>,
    /// Host-side monotonic call-id source.
    ids: Mutex<CallIdSeq>,
    /// The child handle; taken by the waiter task once the plugin dies.
    child: Arc<Mutex<Option<Child>>>,
    /// The plugin's exit status, published by the waiter task.
    exit: watch::Receiver<Option<ExitStatus>>,
    /// Total `pong`s received since spawn, incremented by the reader task.
    /// Liveness accounting for the health checker.
    pongs: Arc<AtomicU64>,
}

impl RunningPlugin {
    /// Spawns the plugin binary at `path`, runs the hello handshake
    /// (validate version + caps, then ack with the host's hello), and returns
    /// a handle ready for calls. A rejected handshake kills the child before
    /// any work happens. Host services are absent, so `host.*` calls answer
    /// `HostUnavailable` / `ConfigUnavailable`.
    pub async fn spawn(path: impl AsRef<Path>) -> Result<RunningPlugin, PluginError> {
        Self::spawn_with(path, None).await
    }

    /// Spawns the plugin binary like [`RunningPlugin::spawn`], but wires the
    /// given host services (Discord I/O seam + config subset) into the reader,
    /// so plugin→host `host.*` calls can be served.
    pub async fn spawn_with(
        path: impl AsRef<Path>,
        host: Option<Arc<HostServices>>,
    ) -> Result<RunningPlugin, PluginError> {
        let path = path.as_ref();
        let label = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("plugin")
            .to_string();

        let mut command = Command::new(path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);

        let mut child = command.spawn().map_err(|source| PluginError::Spawn {
            path: path.to_path_buf(),
            source,
        })?;

        let mut stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let mut reader = BufReader::new(stdout);

        // Handshake: the plugin announces itself first.
        let hello = match read_hello(&label, &mut reader).await {
            Ok(hello) => hello,
            Err(reason) => return Err(reject(child, reason).await),
        };
        let hello = match serde_json::from_str::<Msg>(hello.trim()) {
            Ok(msg) => msg,
            Err(e) => {
                return Err(reject(
                    child,
                    PluginError::HelloLost {
                        name: label,
                        detail: format!("invalid hello json: {e}"),
                    },
                )
                .await);
            }
        };
        let Msg::Hello { v, name, caps } = hello else {
            return Err(reject(
                child,
                PluginError::HelloLost {
                    name: label,
                    detail: format!("first message was not a hello: {hello:?}"),
                },
            )
            .await);
        };
        if let Err(reason) = validate_hello(&name, v, &caps) {
            return Err(reject(child, reason).await);
        }

        // Acknowledge with the host's own hello (nushell-style both-sides
        // hello). Config values are not part of the ack — the `host.get_config`
        // call op serves them later.
        if let Err(source) = write_line(&mut stdin, &host_hello()).await {
            return Err(reject(
                child,
                PluginError::Io {
                    name: name.clone(),
                    source,
                },
            )
            .await);
        }

        let stdin = Arc::new(Mutex::new(Some(stdin)));
        let child = Arc::new(Mutex::new(Some(child)));
        let inflight = Arc::new(Mutex::new(HashMap::new()));
        let (exit_tx, exit_rx) = watch::channel(None);
        let (died_tx, died_rx) = oneshot::channel();
        let pongs = Arc::new(AtomicU64::new(0));

        tokio::spawn(run_stderr(stderr, name.clone()));
        tokio::spawn(run_reader(
            reader,
            inflight.clone(),
            pongs.clone(),
            name.clone(),
            died_tx,
            stdin.clone(),
            host,
        ));
        tokio::spawn(run_reaper(child.clone(), exit_tx, died_rx, name.clone()));

        Ok(RunningPlugin {
            name,
            stdin,
            inflight,
            ids: Mutex::new(CallIdSeq::new()),
            child,
            exit: exit_rx,
            pongs,
        })
    }

    /// Sends a `call` and awaits the correlated `resp`. The call fails with
    /// [`PluginError::CallTimeout`] if the plugin does not answer in time, or
    /// with a `PluginDied` wire error if the plugin dies while in flight.
    pub async fn call(
        &self,
        op: &str,
        cmd: Option<&str>,
        args: Option<Value>,
    ) -> Result<Msg, PluginError> {
        let id = self.ids.lock().await.next_id();
        let (tx, rx) = oneshot::channel();
        self.inflight.lock().await.insert(id, tx);

        let msg = Msg::Call {
            id,
            op: op.to_string(),
            cmd: cmd.map(str::to_string),
            args,
        };
        {
            let mut stdin = self.stdin.lock().await;
            match stdin.as_mut() {
                Some(stdin) => {
                    if let Err(source) = write_line(stdin, &msg).await {
                        self.inflight.lock().await.remove(&id);
                        return Err(PluginError::Io {
                            name: self.name.clone(),
                            source,
                        });
                    }
                }
                None => {
                    self.inflight.lock().await.remove(&id);
                    return Err(PluginError::NotRunning {
                        name: self.name.clone(),
                    });
                }
            }
        }

        match tokio::time::timeout(CALL_TIMEOUT, rx).await {
            Ok(Ok(msg)) => Ok(msg),
            // The sender was dropped without a response — the plugin died
            // while the call was in flight.
            Ok(Err(_)) => Err(PluginError::PluginDied {
                name: self.name.clone(),
            }),
            Err(_) => {
                self.inflight.lock().await.remove(&id);
                Err(PluginError::CallTimeout {
                    name: self.name.clone(),
                    op: op.to_string(),
                    timeout: CALL_TIMEOUT,
                })
            }
        }
    }

    /// Pushes a one-way event to the plugin (e.g. `view.timeout` on session
    /// abandonment), without awaiting a reply. Fails with
    /// [`PluginError::NotRunning`] if the plugin is stopped, or
    /// [`PluginError::Io`] if the write fails.
    pub async fn send_event(&self, name: &str, data: Option<Value>) -> Result<(), PluginError> {
        let msg = Msg::Event {
            name: name.to_string(),
            data,
        };
        let mut stdin = self.stdin.lock().await;
        match stdin.as_mut() {
            Some(stdin) => write_line(stdin, &msg)
                .await
                .map_err(|source| PluginError::Io {
                    name: self.name.clone(),
                    source,
                }),
            None => Err(PluginError::NotRunning {
                name: self.name.clone(),
            }),
        }
    }

    /// Sends a liveness `ping`; the plugin answers with `pong` (no
    /// correlation id). Fails with [`PluginError::NotRunning`] if the plugin
    /// is stopped, or [`PluginError::Io`] if the write fails.
    pub async fn ping(&self) -> Result<(), PluginError> {
        let mut stdin = self.stdin.lock().await;
        match stdin.as_mut() {
            Some(stdin) => write_line(stdin, &Msg::Ping)
                .await
                .map_err(|source| PluginError::Io {
                    name: self.name.clone(),
                    source,
                }),
            None => Err(PluginError::NotRunning {
                name: self.name.clone(),
            }),
        }
    }

    /// Gracefully stops the plugin: sends `bye`, closes stdin (EOF), and
    /// waits up to [`STOP_TIMEOUT`] for a clean exit, killing the child if it
    /// does not comply. Returns the final exit status.
    ///
    /// `stop()` always returns within a bounded time. A plugin that neither
    /// exits cleanly nor dies from the kill within the grace period yields
    /// [`PluginError::StopTimeout`] instead of a hang. The reaper task owns
    /// the final `wait()` on the child; `stop()` only waits on the published
    /// exit status. If the reaper has already taken the child (stdout closed
    /// while the plugin stayed alive), there is nothing left to kill here —
    /// the reaper is already waiting on it — and `stop()` still returns after
    /// the grace period.
    pub async fn stop(&self) -> Result<ExitStatus, PluginError> {
        {
            let mut stdin = self.stdin.lock().await;
            if let Some(stdin) = stdin.as_mut()
                && let Err(e) = write_line(stdin, &Msg::Bye).await
            {
                // The plugin may already be gone; the wait below reports
                // the real outcome.
                warn!("failed to send bye to plugin {}: {e}", self.name);
            }
        }
        // EOF: the plugin exits on stdin EOF even without bye.
        drop(self.stdin.lock().await.take());

        match tokio::time::timeout(STOP_TIMEOUT, self.wait_for_exit()).await {
            Ok(status) => status,
            Err(_) => {
                warn!(
                    "plugin {} did not exit within {STOP_TIMEOUT:?}, killing",
                    self.name
                );
                // The reaper may already own the child (stdout closed while
                // the plugin stayed alive); then it owns the final wait() and
                // there is nothing left to kill here.
                if let Some(child) = self.child.lock().await.as_mut() {
                    child.start_kill().map_err(|source| PluginError::Io {
                        name: self.name.clone(),
                        source,
                    })?;
                }
                // Bounded second wait: if the kill (or the reaper) has not
                // published an exit status in time, report it instead of
                // hanging on a child the reaper may never reap.
                tokio::time::timeout(STOP_TIMEOUT, self.wait_for_exit())
                    .await
                    .map_err(|_| PluginError::StopTimeout {
                        name: self.name.clone(),
                        timeout: STOP_TIMEOUT,
                    })?
            }
        }
    }

    /// The plugin's exit status once it has exited; `None` while it runs.
    pub fn exit_status(&self) -> Option<ExitStatus> {
        *self.exit.borrow()
    }

    /// The child process id, if the child has not yet been reaped.
    pub async fn pid(&self) -> Option<u32> {
        self.child
            .lock()
            .await
            .as_ref()
            .and_then(|child| child.id())
    }

    /// Total `pong`s received since spawn, for liveness accounting.
    pub fn pongs_received(&self) -> u64 {
        self.pongs.load(Ordering::Relaxed)
    }

    /// Awaits the plugin's exit status, published by the waiter task once the
    /// child is reaped.
    async fn wait_for_exit(&self) -> Result<ExitStatus, PluginError> {
        let mut exit = self.exit.clone();
        loop {
            if let Some(status) = *exit.borrow() {
                return Ok(status);
            }
            if exit.changed().await.is_err() {
                // The waiter task ended without a status: nothing more comes.
                return Err(PluginError::PluginDied {
                    name: self.name.clone(),
                });
            }
        }
    }
}

impl Drop for RunningPlugin {
    /// Kills the subprocess on drop so a discarded handle never orphans it.
    /// The child is wrapped in an `Arc<Mutex<Option<Child>>>` shared with the
    /// reaper task, so tokio's `kill_on_drop` cannot fire once that `Arc`
    /// stays alive — the plugin would keep running with nobody to stop it.
    /// The reaper reaps the corpse; graceful unload is
    /// [`RunningPlugin::stop`]. Errors are ignored: the process may already
    /// be gone, or the reaper may be reaping it concurrently.
    fn drop(&mut self) {
        let Ok(mut guard) = self.child.try_lock() else {
            return;
        };
        if let Some(mut child) = guard.take() {
            child.start_kill().ok();
        }
    }
}

/// The host's own hello, sent as the ack after a valid plugin hello. Config
/// values are not part of the ack — the `host.get_config` call op serves them
/// later.
fn host_hello() -> Msg {
    Msg::Hello {
        v: API_VERSION,
        name: "host".into(),
        caps: ALL_CAPS
            .iter()
            .map(|cap| cap.as_str().to_string())
            .collect(),
    }
}

/// Validates a plugin's hello before any work happens: the version must match
/// and every declared `host.*` cap must be in the v1 surface. Rejection is
/// decided here, before any other message is exchanged.
fn validate_hello(name: &str, v: u32, caps: &[String]) -> Result<(), PluginError> {
    if v != API_VERSION {
        return Err(PluginError::VersionMismatch {
            name: name.to_string(),
            got: v,
            expected: API_VERSION,
        });
    }
    validate_caps(caps).map_err(PluginError::Caps)?;
    Ok(())
}

/// Writes one protocol message to the plugin's stdin as a JSON line. Every
/// write is compact JSON + `\n` + flush: the plugin side block-buffers piped
/// stdout, and a missed flush deadlocks the handshake.
async fn write_line(stdin: &mut ChildStdin, msg: &Msg) -> std::io::Result<()> {
    let mut line = serde_json::to_string(msg).expect("serialize protocol message");
    line.push('\n');
    stdin.write_all(line.as_bytes()).await?;
    stdin.flush().await
}

/// Reads the plugin's first stdout line (its hello) within [`HELLO_TIMEOUT`].
async fn read_hello(
    label: &str,
    reader: &mut BufReader<ChildStdout>,
) -> Result<String, PluginError> {
    let mut line = String::new();
    match tokio::time::timeout(HELLO_TIMEOUT, reader.read_line(&mut line)).await {
        Err(_) => Err(PluginError::HelloTimeout {
            name: label.to_string(),
            timeout: HELLO_TIMEOUT,
        }),
        Ok(Err(source)) => Err(PluginError::Io {
            name: label.to_string(),
            source,
        }),
        Ok(Ok(0)) => Err(PluginError::HelloLost {
            name: label.to_string(),
            detail: "stdout closed before the hello".into(),
        }),
        Ok(Ok(_)) => Ok(line),
    }
}

/// Kills and reaps the child after a rejected handshake, then hands back the
/// rejection reason.
async fn reject(mut child: Child, reason: PluginError) -> PluginError {
    child.start_kill().ok();
    let _ = child.wait().await;
    reason
}

/// Forwards the plugin's stderr (its free logging channel) into the host's
/// logs. Draining the pipe also prevents the plugin blocking on a full pipe
/// buffer.
async fn run_stderr(stderr: ChildStderr, name: String) {
    let mut lines = BufReader::new(stderr).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => info!("[plugin {name}] {line}"),
            Ok(None) => break,
            Err(e) => {
                warn!("plugin {name} stderr read error: {e}");
                break;
            }
        }
    }
}

/// Reads the plugin's stdout lines and dispatches them by message type. EOF
/// or a decode error is the death signal: every in-flight call fails with a
/// `PluginDied` wire error and the waiter task is notified to reap the child.
async fn run_reader(
    mut reader: BufReader<ChildStdout>,
    inflight: Arc<Mutex<HashMap<u64, oneshot::Sender<Msg>>>>,
    pongs: Arc<AtomicU64>,
    name: String,
    died: oneshot::Sender<()>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    host: Option<Arc<HostServices>>,
) {
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF: the plugin's stdout closed.
            Ok(_) => match serde_json::from_str::<Msg>(&line) {
                Ok(msg) => dispatch(&msg, &inflight, &pongs, &name, &stdin, host.as_deref()).await,
                Err(e) => {
                    warn!("plugin {name} wrote an invalid protocol line, treating as death: {e}");
                    break;
                }
            },
            Err(e) => {
                warn!("plugin {name} stdout read error, treating as death: {e}");
                break;
            }
        }
    }

    let pending: Vec<(u64, oneshot::Sender<Msg>)> = inflight.lock().await.drain().collect();
    for (id, tx) in pending {
        let _ = tx.send(Msg::resp_err(
            id,
            WireError {
                kind: "PluginDied".into(),
                msg: format!("plugin {name} died"),
            },
        ));
    }
    let _ = died.send(());
}

/// Routes one plugin message. `resp`s are matched to their waiting call by
/// id; plugin→host `host.*` calls are served through the host services and
/// answered on the plugin's stdin; everything else is logged.
async fn dispatch(
    msg: &Msg,
    inflight: &Arc<Mutex<HashMap<u64, oneshot::Sender<Msg>>>>,
    pongs: &Arc<AtomicU64>,
    name: &str,
    stdin: &Arc<Mutex<Option<ChildStdin>>>,
    host: Option<&HostServices>,
) {
    match msg {
        Msg::Resp { id, .. } => {
            if let Some(tx) = inflight.lock().await.remove(id) {
                let _ = tx.send(msg.clone());
            } else {
                debug!("plugin {name} answered id {id} which has no waiting call");
            }
        }
        Msg::Call { id, op, args, .. } => {
            let resp = host::handle_host_call(*id, op, args.as_ref(), host).await;
            let mut guard = stdin.lock().await;
            let Some(mut pipe) = guard.take() else {
                warn!("plugin {name} call `{op}` arrived after stdin closed");
                return;
            };
            drop(guard);
            if let Err(e) = write_line(&mut pipe, &resp).await {
                warn!("plugin {name} call `{op}` failed to answer: {e}");
            }
            *stdin.lock().await = Some(pipe);
        }
        Msg::Event { name: event, .. } => info!("plugin {name} emitted event `{event}`"),
        Msg::Pong => {
            pongs.fetch_add(1, Ordering::Relaxed);
        }
        Msg::Hello { .. } | Msg::Ping | Msg::Bye => {
            warn!("plugin {name} sent unexpected message {msg:?}");
        }
    }
}

/// Awaits the death signal from the reader task, then reaps the child via
/// `wait()` and publishes the exit status. Status 0 is logged as a clean
/// exit; anything else (including signal death) as a crash.
async fn run_reaper(
    child: Arc<Mutex<Option<Child>>>,
    exit: watch::Sender<Option<ExitStatus>>,
    died: oneshot::Receiver<()>,
    name: String,
) {
    let _ = died.await;
    let child = child.lock().await.take();
    let Some(mut child) = child else {
        warn!("plugin {name} was reaped without a child handle");
        let _ = exit.send(None);
        return;
    };
    match child.wait().await {
        Ok(status) => {
            if status.code() == Some(0) {
                info!("plugin {name} exited cleanly: {status}");
            } else {
                warn!("plugin {name} crashed: {status}");
            }
            let _ = exit.send(Some(status));
        }
        Err(e) => {
            warn!("failed to wait for plugin {name}: {e}");
            let _ = exit.send(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use pwr_plugin_protocol::CapsError;

    use super::*;

    // ── hello validation (the reject-before-work decision) ─────────────────

    #[test]
    fn valid_hello_is_accepted() {
        let caps = vec!["command:hello".into(), "host.kv.get".into()];
        assert!(validate_hello("hello", API_VERSION, &caps).is_ok());
    }

    #[test]
    fn version_mismatch_is_rejected_before_any_work() {
        let err = validate_hello("hello", 2, &[]).unwrap_err();
        assert!(matches!(
            err,
            PluginError::VersionMismatch {
                got: 2,
                expected: API_VERSION,
                ..
            }
        ));
    }

    #[test]
    fn unknown_host_cap_is_rejected() {
        let err = validate_hello("hello", API_VERSION, &["host.frobnicate".into()]).unwrap_err();
        assert!(matches!(
            err,
            PluginError::Caps(CapsError { ref op }) if op == "host.frobnicate"
        ));
    }

    // ── the host ack hello ──────────────────────────────────────────────────

    #[test]
    fn host_hello_announces_the_full_cap_surface() {
        let Msg::Hello { v, name, caps } = host_hello() else {
            panic!("host ack must be a hello")
        };
        assert_eq!(v, API_VERSION);
        assert_eq!(name, "host");
        assert_eq!(caps.len(), 9, "every v1 host cap must be announced");
        assert!(caps.iter().any(|c| c == "host.defer"));
        assert!(caps.iter().any(|c| c == "host.kv.get"));
        assert!(caps.iter().any(|c| c == "host.get_config"));
    }

    // ── pong accounting (the health checker's liveness signal) ──────────────

    #[tokio::test]
    async fn pong_increments_the_liveness_counter() {
        let inflight = Arc::new(Mutex::new(HashMap::new()));
        let pongs = Arc::new(AtomicU64::new(0));
        let stdin = Arc::new(Mutex::new(None::<ChildStdin>));
        dispatch(&Msg::Pong, &inflight, &pongs, "hello", &stdin, None).await;
        assert_eq!(pongs.load(Ordering::Relaxed), 1);
    }
}
