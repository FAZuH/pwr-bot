//! Managed background service for pwr-bot.
//!
//! The service is the same binary launched as `serve --service` in its own
//! session. A 0600 registration file is the lease: the server re-reads it as a
//! watchdog, and clients use it to find the live pid.

use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::ErrorKind;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use clap::Subcommand;
use serde::Deserialize;
use serde::Serialize;
use tokio::time::sleep;
use uuid::Uuid;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registration {
    pub id: String,
    pub version: String,
    pub pid: u32,
    /// Absolute log directory, so a tail from any CWD follows the same service.
    #[serde(default)]
    pub logs_path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub env_file: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum ServiceCommand {
    /// Start the detached service, reusing a healthy one
    Start,
    /// Stop the service and clear the lease
    Stop,
    /// Stop, then start the service
    Restart,
    /// Report whether the registered service is alive
    Status,
    /// Store the env file used by every detached spawn
    Install {
        /// File with KEY=VALUE lines, read on spawn and injected into the child
        #[arg(long)]
        env_file: PathBuf,
    },
}

fn var_or(key: &str, default: impl Into<PathBuf>) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| default.into())
}

fn home() -> PathBuf {
    var_or("HOME", ".")
}

/// State directory for the registration lease.
pub fn state_dir() -> PathBuf {
    var_or("XDG_STATE_HOME", home().join(".local/state")).join("pwr-bot")
}

/// Private config directory for CLI-owned service settings.
pub fn config_dir() -> PathBuf {
    var_or("XDG_CONFIG_HOME", home().join(".config")).join("pwr-bot")
}

pub fn registration_path() -> PathBuf {
    state_dir().join("service.json")
}

pub fn service_config_path() -> PathBuf {
    config_dir().join("service.json")
}

pub fn data_path() -> PathBuf {
    var_or("DATA_PATH", "./data")
}

pub fn logs_path() -> PathBuf {
    var_or("LOGS_PATH", "./logs")
}

pub fn print_paths() {
    let log = read_registration(&registration_path())
        .ok()
        .flatten()
        .map(|reg| reg.logs_path)
        .filter(|path| path.is_absolute() && !path.as_os_str().is_empty())
        .unwrap_or_else(|| {
            crate::config::canonical_dir(&logs_path()).unwrap_or_else(|_| logs_path())
        });
    println!("config:       {}", service_config_path().display());
    println!("data:         {}", data_path().display());
    println!("log:          {}", log.display());
    println!("registration: {}", registration_path().display());
}

fn atomic_write(path: &Path, suffix: &str, text: &str) -> Result<()> {
    let parent = path.parent().context("path has no parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create `{}`", parent.display()))?;
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(suffix);
    let tmp = PathBuf::from(tmp);
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("failed to open `{}`", tmp.display()))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set permissions on `{}`", tmp.display()))?;
    file.write_all(text.as_bytes())
        .with_context(|| format!("failed to write `{}`", tmp.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync `{}`", tmp.display()))?;
    drop(file);
    fs::rename(&tmp, path).with_context(|| {
        format!(
            "failed to rename `{}` to `{}`",
            tmp.display(),
            path.display()
        )
    })?;
    Ok(())
}

pub fn write_registration(path: &Path, reg: &Registration) -> Result<()> {
    let text = serde_json::to_string(reg).context("failed to serialize registration")?;
    atomic_write(path, &format!(".{}.tmp", reg.id), &text)
}

pub fn read_registration(path: &Path) -> Result<Option<Registration>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e).with_context(|| format!("failed to read `{}`", path.display()));
        }
    };
    let reg = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse `{}`", path.display()))?;
    Ok(Some(reg))
}

/// Removes the lease only when `id` still owns it. Returns whether it removed.
///
/// The lease is renamed aside before the id is re-checked, so a writer that
/// atomically replaces it between the two reads cannot have its file unlinked.
pub fn remove_registration_if_owner(path: &Path, id: &str) -> Result<bool> {
    let Some(reg) = read_registration(path)? else {
        return Ok(false);
    };
    if reg.id != id {
        return Ok(false);
    }

    let mut claimed_name = path.as_os_str().to_os_string();
    claimed_name.push(format!(".{id}.removing"));
    let claimed = PathBuf::from(claimed_name);
    match fs::rename(path, &claimed) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).with_context(|| format!("failed to claim `{}`", path.display())),
    }

    let still_owned = fs::read_to_string(&claimed)
        .ok()
        .and_then(|text| serde_json::from_str::<Registration>(&text).ok())
        .is_some_and(|moved| moved.id == id);

    if still_owned {
        match fs::remove_file(&claimed) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(true),
            Err(e) => Err(e).with_context(|| format!("failed to remove `{}`", claimed.display())),
        }
    } else {
        let _ = fs::hard_link(&claimed, path);
        let _ = fs::remove_file(&claimed);
        Ok(false)
    }
}

pub fn read_service_config() -> Result<Option<ServiceConfig>> {
    let path = service_config_path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e).with_context(|| format!("failed to read `{}`", path.display()));
        }
    };
    let config = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse `{}`", path.display()))?;
    Ok(Some(config))
}

fn write_service_config(config: &ServiceConfig) -> Result<()> {
    let text =
        serde_json::to_string_pretty(config).context("failed to serialize service config")?;
    atomic_write(&service_config_path(), ".tmp", &text)
}

/// Parses `KEY=VALUE` lines. Blank lines and `#` comments are ignored; an
/// `export ` prefix and surrounding quotes are optional. Malformed lines are
/// errors so a typo cannot silently start a service without its settings.
pub fn parse_env_file(text: &str) -> Result<Vec<(String, String)>> {
    let mut vars = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, value)) = line.split_once('=') else {
            bail!("env line {} has no `=`: {line:?}", index + 1);
        };
        let key = key.trim();
        if key.is_empty() {
            bail!("env line {} has an empty key", index + 1);
        }
        let value = value.trim();
        let value = if let Some(inner) = value.strip_prefix('"') {
            inner
                .strip_suffix('"')
                .with_context(|| format!("env line {} has an unmatched quote", index + 1))?
                .to_string()
        } else if let Some(inner) = value.strip_prefix('\'') {
            inner
                .strip_suffix('\'')
                .with_context(|| format!("env line {} has an unmatched quote", index + 1))?
                .to_string()
        } else {
            if value.contains('#') {
                bail!(
                    "env line {} has an unquoted `#`; quote the value",
                    index + 1
                );
            }
            value.to_string()
        };
        vars.push((key.to_string(), value));
    }
    Ok(vars)
}

pub fn read_env_file(path: &Path) -> Result<Vec<(String, String)>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read env file `{}`", path.display()))?;
    parse_env_file(&text)
}

/// Builds the detached child command. Secrets are set as environment entries,
/// never as arguments, and `setsid` runs in the forked child before exec.
fn build_service_command(exe: &Path, env_vars: &[(String, String)]) -> Command {
    let mut command = Command::new(exe);
    command.args(["serve", "--service"]);
    command.stdin(Stdio::null()).stdout(Stdio::null());
    for (key, value) in env_vars {
        command.env(key, value);
    }
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    command
}

fn spawn_detached() -> Result<u32> {
    let exe = env::current_exe().context("failed to resolve the current executable")?;
    let env_vars = read_service_config()?
        .and_then(|config| config.env_file)
        .map(|path| read_env_file(&path))
        .transpose()?
        .unwrap_or_default();

    let date = chrono::Utc::now().format("%Y-%m-%d");
    let stderr_path = logs_path().join(format!("pwr-bot.{date}.log"));
    if let Some(parent) = stderr_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    let stderr = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(&stderr_path)
        .with_context(|| format!("failed to open `{}`", stderr_path.display()))?;

    let mut command = build_service_command(&exe, &env_vars);
    command.stderr(Stdio::from(stderr));
    let child = command.spawn().context("failed to spawn the service")?;
    let pid = child.id();
    drop(child);
    Ok(pid)
}

pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as i32, 0) };
    rc == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

async fn wait_for_registration(previous_id: Option<&str>) -> Result<Registration> {
    let path = registration_path();
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if let Some(reg) = read_registration(&path)? {
            let is_new = previous_id.is_none_or(|old| reg.id != old);
            if is_new && pid_alive(reg.pid) {
                return Ok(reg);
            }
        }
        if Instant::now() >= deadline {
            bail!(
                "service did not register within {}s",
                STARTUP_TIMEOUT.as_secs()
            );
        }
        sleep(POLL_INTERVAL).await;
    }
}

/// Starts a service, reusing a healthy one and replacing a mismatched one.
pub async fn ensure() -> Result<Registration> {
    let path = registration_path();
    let previous_id = match read_registration(&path)? {
        Some(reg) if pid_alive(reg.pid) => {
            if reg.version == VERSION {
                println!(
                    "service already running (pid {}, version {})",
                    reg.pid, reg.version
                );
                return Ok(reg);
            }
            println!(
                "service version {} does not match {}; replacing",
                reg.version, VERSION
            );
            stop_at(&path, Some(&reg)).await?;
            Some(reg.id)
        }
        Some(reg) => {
            let _ = remove_registration_if_owner(&path, &reg.id)?;
            None
        }
        None => None,
    };
    spawn_detached()?;
    let reg = wait_for_registration(previous_id.as_deref()).await?;
    println!("service started (pid {}, version {})", reg.pid, reg.version);
    Ok(reg)
}

pub async fn start() -> Result<()> {
    ensure().await?;
    Ok(())
}

pub async fn stop() -> Result<()> {
    stop_at(&registration_path(), None).await
}

/// Signals only the lease the caller decided to stop. When `expected` is set,
/// a concurrent registration change aborts instead of signalling a new owner.
async fn stop_at(path: &Path, expected: Option<&Registration>) -> Result<()> {
    let reg = match expected {
        Some(reg) => reg.clone(),
        None => match read_registration(path)? {
            Some(reg) => reg,
            None => {
                println!("service is not running");
                return Ok(());
            }
        },
    };

    match read_registration(path)? {
        Some(current) if current.id == reg.id && current.pid == reg.pid => {}
        _ => {
            println!("service lease changed; refusing to signal pid {}", reg.pid);
            return Ok(());
        }
    }

    if !pid_alive(reg.pid) {
        let _ = remove_registration_if_owner(path, &reg.id)?;
        println!("service is not running; removed stale registration");
        return Ok(());
    }

    unsafe {
        libc::kill(reg.pid as i32, libc::SIGTERM);
    }
    let deadline = Instant::now() + STOP_TIMEOUT;
    while pid_alive(reg.pid) && Instant::now() < deadline {
        sleep(POLL_INTERVAL).await;
    }
    if pid_alive(reg.pid) {
        unsafe {
            libc::kill(reg.pid as i32, libc::SIGKILL);
        }
        let deadline = Instant::now() + POLL_INTERVAL * 20;
        while pid_alive(reg.pid) && Instant::now() < deadline {
            sleep(POLL_INTERVAL).await;
        }
    }
    let removed = remove_registration_if_owner(path, &reg.id)?;
    println!(
        "service stopped (pid {}{})",
        reg.pid,
        if removed {
            ""
        } else {
            "; lease no longer owned"
        }
    );
    Ok(())
}

pub async fn restart() -> Result<()> {
    stop().await?;
    start().await
}

pub fn status() -> Result<()> {
    let path = registration_path();
    let Some(reg) = read_registration(&path)? else {
        println!("service is not running");
        return Ok(());
    };
    if pid_alive(reg.pid) {
        println!(
            "service is running (pid {}, version {}, id {})",
            reg.pid, reg.version, reg.id
        );
    } else {
        println!(
            "service is not running (stale registration for pid {}, id {})",
            reg.pid, reg.id
        );
    }
    Ok(())
}

pub fn install(env_file: &Path) -> Result<()> {
    let canonical = env_file
        .canonicalize()
        .with_context(|| format!("env file `{}` does not exist", env_file.display()))?;
    write_service_config(&ServiceConfig {
        env_file: Some(canonical),
    })?;
    println!(
        "service env file stored at {}",
        service_config_path().display()
    );
    Ok(())
}

pub fn new_registration(logs_path: &Path) -> Registration {
    Registration {
        id: Uuid::new_v4().to_string(),
        version: VERSION.to_string(),
        pid: std::process::id(),
        logs_path: logs_path.to_path_buf(),
    }
}

/// Waits for Ctrl+C or SIGTERM. Callers run their shutdown actions after this
/// returns so plugin process groups, the lease, and the log guard are released.
pub async fn wait_for_signal() -> &'static str {
    let Ok(mut sigterm) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        log::warn!("SIGTERM handler unavailable; waiting for Ctrl+C only");
        let _ = tokio::signal::ctrl_c().await;
        return "Ctrl+C received";
    };
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            if let Err(e) = result {
                log::warn!("ctrl_c error: {e}");
            }
            "Ctrl+C received"
        }
        _ = sigterm.recv() => "SIGTERM received",
    }
}

/// Shuts the service down if another process took over the lease.
pub async fn watchdog(owned: Registration) {
    loop {
        sleep(WATCHDOG_INTERVAL).await;
        match read_registration(&registration_path()) {
            Ok(Some(current)) if current == owned => continue,
            Ok(other) => {
                log::warn!(
                    "service lease changed ({:?}); shutting down",
                    other.is_some()
                );
                return;
            }
            Err(e) => {
                log::warn!("failed to re-read service lease: {e:#}; shutting down");
                return;
            }
        }
    }
}

pub fn release(reg: &Registration) {
    if let Err(e) = remove_registration_if_owner(&registration_path(), &reg.id) {
        log::warn!("failed to release service lease: {e:#}");
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::time::Duration;

    use super::*;

    fn registration(id: &str, logs_path: &Path) -> Registration {
        Registration {
            id: id.to_string(),
            version: VERSION.to_string(),
            pid: std::process::id(),
            logs_path: logs_path.to_path_buf(),
        }
    }

    #[test]
    fn registration_roundtrip_with_0600_mode_and_absolute_log_dir() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("service.json");
        let reg = registration(
            "b7f8f2a0-9c8e-4a1e-9c1a-6a8b8e1b2f3d",
            Path::new("/var/log/pwr-bot"),
        );

        write_registration(&path, &reg).expect("write registration");
        assert_eq!(
            read_registration(&path).expect("read registration"),
            Some(reg)
        );
        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn stale_lease_cannot_be_removed_by_another_owner() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("service.json");
        let reg = registration("owner-a", dir.path());

        write_registration(&path, &reg).expect("write registration");
        assert!(!remove_registration_if_owner(&path, "owner-b").expect("reject owner-b"));
        assert!(path.exists());
        assert!(remove_registration_if_owner(&path, "owner-a").expect("accept owner-a"));
        assert!(!path.exists());
        assert!(!dir.path().join("service.json.owner-a.removing").exists());
    }

    #[tokio::test]
    async fn stop_refuses_to_signal_a_replaced_lease() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("service.json");
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep stub");
        let owner = Registration {
            pid: child.id(),
            ..registration("owner", dir.path())
        };
        write_registration(&path, &owner).expect("write lease");
        let stale = Registration {
            id: "stale".to_string(),
            ..owner
        };

        stop_at(&path, Some(&stale)).await.expect("stop");
        sleep(Duration::from_millis(100)).await;
        assert!(
            child.try_wait().expect("child status").is_none(),
            "the new owner's process must survive"
        );
        assert!(path.exists(), "the new owner's lease must survive");
        child.kill().expect("cleanup stub");
        let _ = child.wait();
    }

    #[tokio::test]
    async fn stop_signals_the_owned_process_and_clears_the_lease() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("service.json");
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep stub");
        let owner = Registration {
            pid: child.id(),
            ..registration("owner", dir.path())
        };
        write_registration(&path, &owner).expect("write lease");

        stop_at(&path, Some(&owner)).await.expect("stop");
        let status = child.wait().expect("wait stub");
        assert!(!status.success(), "SIGTERM must end the owned process");
        assert!(!path.exists(), "the owned lease must be removed");
    }

    #[test]
    fn env_file_parsing_ignores_comments_and_unquotes_values() {
        let vars = parse_env_file(
            "# injected at spawn\n\nDISCORD_TOKEN=\"super-secret\"\nexport ADMIN_ID='123'\n",
        )
        .expect("parse env file");
        assert_eq!(
            vars,
            vec![
                ("DISCORD_TOKEN".to_string(), "super-secret".to_string()),
                ("ADMIN_ID".to_string(), "123".to_string()),
            ]
        );
    }

    #[test]
    fn env_file_parsing_rejects_malformed_lines() {
        assert!(parse_env_file("DISCORD_TOKEN\n").is_err());
        assert!(parse_env_file("=orphan\n").is_err());
        assert!(parse_env_file("DISCORD_TOKEN=\"unclosed\n").is_err());
        assert!(parse_env_file("DISCORD_TOKEN=abc # trailing\n").is_err());
    }

    #[tokio::test]
    async fn sigterm_is_treated_as_a_shutdown_signal() {
        const CHILD: &str = "PWRBOT_SIGNAL_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let reason = tokio::time::timeout(Duration::from_secs(3), wait_for_signal())
                .await
                .expect("signal received");
            assert_eq!(reason, "SIGTERM received");
            return;
        }
        let mut child = std::process::Command::new(std::env::current_exe().expect("test exe"))
            .arg("--exact")
            .arg("cli::service::tests::sigterm_is_treated_as_a_shutdown_signal")
            .env(CHILD, "1")
            .spawn()
            .expect("spawn signal child");
        sleep(Duration::from_millis(500)).await;
        assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
        let status = child.wait().expect("wait signal child");
        assert!(status.success(), "child must observe SIGTERM; {status}");
    }

    #[test]
    fn spawn_command_keeps_secrets_out_of_argv() {
        let vars = vec![("DISCORD_TOKEN".to_string(), "super-secret".to_string())];
        let command = build_service_command(Path::new("/usr/bin/pwr-bot"), &vars);
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, vec![OsStr::new("serve"), OsStr::new("--service")]);
        assert!(
            args.iter()
                .all(|arg| !arg.to_string_lossy().contains("super-secret"))
        );

        let envs: Vec<_> = command.get_envs().collect();
        assert!(envs.iter().any(|(key, value)| {
            *key == OsStr::new("DISCORD_TOKEN") && *value == Some(OsStr::new("super-secret"))
        }));
    }
}
