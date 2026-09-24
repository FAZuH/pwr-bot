//! Resolves a PostgreSQL connection URL for tests, self-provisioning when
//! the environment's URL is unreachable.
//!
//! Selection (resolved once per process, cached):
//! 1. The effective configured URL (`DB_URL` env var, then the `.env` file
//!    via dotenv, then the CI fallback `postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot`)
//!    is probed with a real client connection.
//! 2. Reachable (CI service postgres, local container): the process gets a
//!    dedicated database `pwr_bot_test_<pid>` on that server, recreated from
//!    scratch. Integration test binaries run concurrently in one cargo test
//!    invocation; sharing one database would let one binary's cleanup
//!    truncate another binary's rows mid-test. The dedicated per-process
//!    database removes cross-binary contamination without serializing
//!    binaries. Cleanup semantics are unchanged (`delete_all_tables`) but
//!    now scoped to this process's own database.
//! 3. Unreachable (dead or auth-rejecting server — a TCP-only check is not
//!    enough): an embedded PostgreSQL 17 cluster (matching CI's
//!    `postgres:17-alpine`) is started for this process: ephemeral port,
//!    temporary data dir, Zonky static binaries downloaded once into
//!    `$TMPDIR/pwr-bot-pg-embedded`, database `pwr_bot`. The whole cluster
//!    is process-local, so no cross-binary sharing is possible either.
//!
//! The URL's role must own CREATEDB for path 2 (true for CI's
//! `POSTGRES_USER=pwr_bot` superuser and the dev container).
//!
//! Orphan guards: normal process exit runs `atexit` hooks that SIGTERM an
//! embedded postmaster (PID read from `postmaster.pid`) so a failing test
//! does not leak it between runs, and force-drops this process's scratch
//! database on the external server.

use std::process::id;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::time::Duration;

use postgresql_embedded::PostgreSQL;
use postgresql_embedded::Settings;
use postgresql_embedded::VersionReq;
use tokio::sync::Mutex;
use tokio_postgres_rustls::MakeRustlsConnect;

/// PostgreSQL version matching CI's `postgres:17-alpine`.
const PG_VERSION_REQ: &str = "=17";

/// Fallback URL, identical to CI's service contract and `.env-example`.
const FALLBACK_URL: &str = "postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot";

/// Database name inside an embedded cluster (process-local by construction).
const EMBEDDED_DATABASE: &str = "pwr_bot";

/// Connect timeout for the reachability probe.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// RAII wrapper locking a file for the duration of embedded setup.
struct Fd(std::fs::File);

impl Fd {
    fn lock_exclusive(&mut self) {
        use std::os::fd::AsRawFd;
        let rc = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_EX) };
        assert_eq!(rc, 0, "acquire embedded install lock");
    }

    fn unlock(&mut self) {
        use std::os::fd::AsRawFd;
        let rc = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
        assert_eq!(rc, 0, "release embedded install lock");
    }
}

/// postmaster PID captured after a successful embedded start (0 = none).
static EMBEDDED_POSTMASTER_PID: AtomicU32 = AtomicU32::new(0);

/// Scratch database this process created on the external server, for the
/// best-effort drop at exit. `None` on the embedded path.
static EXTERNAL_SCRATCH: OnceLock<(String, String)> = OnceLock::new();

/// Resolved connection URL; `None` until the first `db_url()` call.
static SELECTED: OnceLock<String> = OnceLock::new();

/// Serializes first-resolution so exactly one probe/embedded start happens.
static SELECTION: Mutex<()> = Mutex::const_new(());

extern "C" fn terminate_embedded_postmaster() {
    let pid = EMBEDDED_POSTMASTER_PID.load(Ordering::SeqCst);
    if pid != 0 {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
    }
}

/// Best-effort `DROP DATABASE ... WITH (FORCE)` for this process's scratch
/// database, run from an `atexit` hook. A fresh single-thread runtime is
/// built because no runtime is alive at exit. Failures are ignored: CI
/// servers are ephemeral and a leftover scratch DB never blocks a later run
/// (each run force-drops its own PID-named database before recreating it).
extern "C" fn drop_external_scratch_database() {
    let Some((maintenance_url, database)) = EXTERNAL_SCRATCH.get() else {
        return;
    };

    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    runtime.block_on(async move {
        // Same connection policy as the probe: TLS when the maintenance URL
        // demands it, plaintext otherwise.
        let Ok(client) = connect(maintenance_url).await else {
            return;
        };
        let _ = client
            .execute(
                &format!("DROP DATABASE IF EXISTS {database} WITH (FORCE)"),
                &[],
            )
            .await;
        drop(client);
    });
}

/// Per-process singleton: the started embedded server.
struct Embedded {
    server: Mutex<Option<PostgreSQL>>,
}

static EMBEDDED: OnceLock<Arc<Embedded>> = OnceLock::new();

fn embedded() -> Arc<Embedded> {
    EMBEDDED
        .get_or_init(|| {
            Arc::new(Embedded {
                server: Mutex::new(None),
            })
        })
        .clone()
}

/// Returns the process's test connection URL, resolving it exactly once.
///
/// Later calls return the cached URL without re-probing, so a process can
/// never switch between the external and embedded path mid-run.
pub async fn db_url() -> String {
    if let Some(url) = SELECTED.get() {
        return url.clone();
    }

    let _guard = SELECTION.lock().await;

    if let Some(url) = SELECTED.get() {
        return url.clone();
    }

    let url = resolve().await;
    SELECTED
        .set(url.clone())
        .expect("test DB URL set exactly once");
    url
}

/// One-shot selection: external per-process database, else embedded cluster.
async fn resolve() -> String {
    let configured = effective_configured_url();
    let tls = tls_policy(&configured);

    match tls {
        TlsPolicy::UnsupportedMode(mode) => panic!(
            "configured DB URL requests '{mode}', which this test harness does not support \
             (supported: no sslmode, sslmode=disable, sslmode=require); \
             refusing to connect and refusing to fall back to the embedded server"
        ),
        TlsPolicy::PlainText => {
            if probe_connectable(&configured).await {
                return external_scratch(&configured).await;
            }
            start_embedded().await
        }
        TlsPolicy::Required => {
            match connect(&configured).await {
                // The TLS handshake (with certificate verification against
                // the OS trust store) happens inside `connect`, so a
                // successful probe proves the server's certificate is valid.
                Ok(_) => external_scratch(&configured).await,
                // Never silently downgrade a TLS-requiring URL to embedded.
                Err(error) => panic!(
                    "configured DB URL requires TLS but the connection failed: {error}; \
                     refusing to fall back to the embedded server"
                ),
            }
        }
    }
}

/// Provisions the per-process scratch database on the reachable external
/// server described by `configured` and returns its URL.
async fn external_scratch(configured: &str) -> String {
    let database = format!("pwr_bot_test_{}", id());
    let admin_url = with_database_name(configured, "postgres");

    recreate_database(&admin_url, &database).await;

    if EXTERNAL_SCRATCH
        .set((admin_url.clone(), database.clone()))
        .is_ok()
    {
        unsafe {
            libc::atexit(drop_external_scratch_database);
        }
    }

    with_database_name(configured, &database)
}

/// TLS policy for the effective configured URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TlsPolicy {
    /// No sslmode parameter or `sslmode=disable`: plaintext is correct.
    PlainText,
    /// `sslmode=require`: the connection must use verified TLS.
    Required,
    /// verify-ca/verify-full: tokio-postgres cannot express them; refuse.
    UnsupportedMode(&'static str),
}

/// Classifies `url`'s sslmode parameter. The CI fallback and the dev
/// container URLs carry no sslmode (plain), so CI/local behavior is
/// unchanged; only URLs that explicitly ask for TLS take the TLS path.
fn tls_policy(url: &str) -> TlsPolicy {
    match sslmode(url).as_deref() {
        None | Some("disable") | Some("prefer") => TlsPolicy::PlainText,
        Some("require") => TlsPolicy::Required,
        Some("verify-ca") | Some("verify-full") => {
            TlsPolicy::UnsupportedMode("verify-ca or verify-full")
        }
        Some(_) => TlsPolicy::UnsupportedMode("an unsupported sslmode"),
    }
}

/// Returns the `sslmode` query parameter of `url`, if present.
fn sslmode(url: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    let query = query.split('#').next().unwrap_or(query);
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "sslmode").then(|| value.to_string())
    })
}

/// Connects to `url` with the connector its sslmode demands: verified TLS
/// via the OS trust store when the URL requires TLS, plaintext otherwise.
/// The connection future is spawned on the current runtime, so the caller
/// only has to drop the returned client.
///
/// `sslmode=prefer` behaves like plaintext here: tokio-postgres' default
/// would attempt TLS but allow sessions without it, and the test servers
/// this harness targets (CI service, local container, embedded) are all
/// plaintext, so no TLS-capable connector is needed for that mode.
async fn connect(url: &str) -> Result<tokio_postgres::Client, tokio_postgres::Error> {
    if tls_policy(url) == TlsPolicy::Required {
        let Ok((tls, errors)) = MakeRustlsConnect::with_native_certs() else {
            // No OS certificates could be loaded: verified TLS is
            // impossible, so fail rather than downgrade.
            panic!("no TLS root certificates available from the OS trust store");
        };
        if !errors.is_empty() {
            eprintln!("TLS trust-store load warnings: {errors:?}");
        }
        let (client, connection) = tokio_postgres::connect(url, tls).await?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        return Ok(client);
    }

    let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls).await?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

/// Effective configured URL: `DB_URL` env var, then the `.env` file via
/// dotenv, then the CI fallback.
///
/// The `.env` value is read directly from the file (public `dotenv::Iter`)
/// instead of relying on `from_filename` + re-reading the env var: dotenv's
/// `load()` skips keys that are already set, so an explicitly *empty*
/// `DB_URL` in the environment would otherwise block loading the file's
/// non-empty value and incorrectly select the fallback.
fn effective_configured_url() -> String {
    let from_env = std::env::var("DB_URL").ok();
    // The iterator API is deprecated by dotenv's own recommendation, which
    // points at `from_path` + `var` — exactly the set-var-then-read pattern
    // that breaks here. It is the only way to read a .env value without the
    // set-if-unset side effect.
    #[allow(deprecated)]
    let from_dotenv = dotenv::from_path_iter(".env").ok().and_then(|lines| {
        lines
            .flatten()
            .find(|(key, _)| key == "DB_URL")
            .map(|(_, value)| value)
    });
    configured_url_from_parts(from_env, from_dotenv)
}

/// Pure selection policy: non-empty env var beats non-empty `.env` value,
/// which beats the CI fallback; empty strings count as unset.
fn configured_url_from_parts(env: Option<String>, dotenv: Option<String>) -> String {
    env.filter(|s| !s.is_empty())
        .or(dotenv.filter(|s| !s.is_empty()))
        .unwrap_or_else(|| FALLBACK_URL.to_string())
}

/// Replaces (or appends) the database name in a `postgres://` URL.
///
/// The authority ends at the first `/` after the scheme; credentials live
/// before it and are preserved verbatim (including percent-encoding). A
/// query component (`?sslmode=...`) and fragment are preserved unchanged —
/// only the path segment (database name) is rewritten.
fn with_database_name(url: &str, database: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return format!("{url}/{database}");
    };
    let after_scheme = &url[scheme_end + 3..];
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let after_authority = &after_scheme[authority_end..];

    if after_authority.starts_with(['?', '#']) {
        return format!(
            "{}://{}/{database}{after_authority}",
            &url[..scheme_end],
            authority
        );
    }

    // Path present: keep any query/fragment that follows the first segment.
    let path_and_more = &after_authority[1.min(after_authority.len())..];
    let suffix = path_and_more
        .find(['?', '#'])
        .map(|index| &path_and_more[index..])
        .unwrap_or("");

    format!(
        "{}://{}/{}{}",
        &url[..scheme_end],
        authority,
        database,
        suffix
    )
}

/// True when a real client connection (including auth) succeeds against
/// `url`, using the TLS connector the URL demands.
async fn probe_connectable(url: &str) -> bool {
    matches!(
        tokio::time::timeout(PROBE_TIMEOUT, connect(url)).await,
        Ok(Ok(_)),
    )
}
/// Drops (force-closing foreign connections) and re-creates `database` via
/// `admin_url`, yielding a fresh empty schema. Uses the same connection
/// policy as the probe: TLS when the URL demands it, plaintext otherwise.
async fn recreate_database(admin_url: &str, database: &str) {
    let client = connect(admin_url)
        .await
        .expect("connect for database recreation");

    // PG 13+: WITH (FORCE) terminates other backends on the database. Fall
    // back to a plain drop for older servers.
    if client
        .execute(
            &format!("DROP DATABASE IF EXISTS {database} WITH (FORCE)"),
            &[],
        )
        .await
        .is_err()
    {
        let _ = client
            .execute(&format!("DROP DATABASE IF EXISTS {database}"), &[])
            .await;
    }
    client
        .execute(&format!("CREATE DATABASE {database}"), &[])
        .await
        .expect("create per-process test database");

    drop(client);
}

/// Starts the embedded cluster for this process, creates the scratch
/// database, and returns its connection URL.
async fn start_embedded() -> String {
    let shared = embedded();
    let mut server_slot = shared.server.lock().await;

    // The embedded cluster has exactly one role: the bootstrap superuser
    // `postgres` with `password` (initdb ignores `username` for role
    // creation), so the test URL authenticates as that role.
    let settings = Settings {
        version: VersionReq::parse(PG_VERSION_REQ).expect("valid version requirement"),
        host: "127.0.0.1".to_string(),
        port: 0,
        username: "postgres".to_string(),
        password: "pwr_bot".to_string(),
        temporary: true,
        installation_dir: std::env::temp_dir().join("pwr-bot-pg-embedded"),
        releases_url: "https://github.com/zonkyio/embedded-postgres-binaries".to_string(),
        ..Settings::default()
    };

    let mut server = PostgreSQL::new(settings);

    // Concurrent test binaries resolve the same installation dir; serialize
    // the download/extract so one binary cannot observe a half-extracted
    // tree. The directory lock is held only for setup, not for the server's
    // lifetime.
    let lock_dir = std::env::temp_dir().join("pwr-bot-pg-embedded.lock");
    std::fs::create_dir_all(&lock_dir).expect("create install lock dir");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_dir.join("lock"))
        .expect("open install lock file");
    let mut fd = Fd(lock_file);
    fd.lock_exclusive();
    server
        .setup()
        .await
        .expect("embedded postgres setup (download/install/initdb)");
    fd.unlock();

    server.start().await.expect("embedded postgres start");
    server
        .create_database(EMBEDDED_DATABASE)
        .await
        .expect("embedded postgres scratch database");

    let url = server.settings().url(EMBEDDED_DATABASE);
    capture_postmaster_pid(&server);
    // Keep the handle alive for the process lifetime: dropping it stops the
    // temporary server.
    *server_slot = Some(server);

    url
}

/// Registers the `atexit` orphan guard with the running postmaster's PID.
fn capture_postmaster_pid(server: &PostgreSQL) {
    let Ok(content) = std::fs::read_to_string(server.settings().data_dir.join("postmaster.pid"))
    else {
        return;
    };
    let Ok(pid) = content
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .parse::<u32>()
    else {
        return;
    };

    EMBEDDED_POSTMASTER_PID.store(pid, Ordering::SeqCst);
    unsafe {
        libc::atexit(terminate_embedded_postmaster);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_database_name_replaces_the_path_segment() {
        assert_eq!(
            with_database_name("postgres://u:p@localhost:5432/pwr_bot", "t1"),
            "postgres://u:p@localhost:5432/t1"
        );
    }

    #[test]
    fn with_database_name_preserves_percent_encoded_credentials() {
        assert_eq!(
            with_database_name(
                "postgres://pwr_bot:uCg%2FJMT%2BRz%3D@localhost:5432/pwr_bot",
                "t2"
            ),
            "postgres://pwr_bot:uCg%2FJMT%2BRz%3D@localhost:5432/t2"
        );
    }

    #[test]
    fn with_database_name_appends_when_url_has_no_database() {
        assert_eq!(
            with_database_name("postgres://u:p@db.internal.example:5432", "t3"),
            "postgres://u:p@db.internal.example:5432/t3"
        );
        assert_eq!(
            with_database_name("postgresql://u:p@h", "t4"),
            "postgresql://u:p@h/t4"
        );
    }

    #[test]
    fn with_database_name_handles_bracketed_ipv6_authorities() {
        assert_eq!(
            with_database_name("postgres://u:p@[::1]:5432/db", "t5"),
            "postgres://u:p@[::1]:5432/t5"
        );
    }

    #[test]
    fn with_database_name_preserves_query_parameters() {
        assert_eq!(
            with_database_name(
                "postgres://u:p@localhost:5432/pwr_bot?sslmode=require",
                "t6"
            ),
            "postgres://u:p@localhost:5432/t6?sslmode=require"
        );
        assert_eq!(
            with_database_name(
                "postgres://u:p@localhost:5432/pwr_bot?sslmode=require&application_name=pwr-bot",
                "t7"
            ),
            "postgres://u:p@localhost:5432/t7?sslmode=require&application_name=pwr-bot"
        );
    }

    #[test]
    fn with_database_name_appends_database_before_query_when_url_has_no_path() {
        assert_eq!(
            with_database_name("postgres://u:p@localhost:5432?sslmode=disable", "t8"),
            "postgres://u:p@localhost:5432/t8?sslmode=disable"
        );
    }

    #[test]
    fn sslmode_extraction_handles_query_shapes() {
        assert_eq!(
            sslmode("postgres://u:p@h:5432/db?sslmode=require"),
            Some("require".into())
        );
        assert_eq!(
            sslmode("postgres://u:p@h:5432/db?sslmode=require&application_name=pwr-bot"),
            Some("require".into())
        );
        assert_eq!(
            sslmode("postgres://u:p@h:5432/db?application_name=pwr-bot&sslmode=require"),
            Some("require".into())
        );
        assert_eq!(
            sslmode("postgres://u:p@h:5432/db?sslmode=require#frag"),
            Some("require".into()),
            "fragment must not leak into the query"
        );
        assert_eq!(sslmode("postgres://u:p@h:5432/db"), None);
        assert_eq!(sslmode("postgres://u:p@h:5432/db?application_name=x"), None);
        assert_eq!(sslmode("postgres://u:p@h:5432/db?notsslmode=require"), None);
    }

    #[test]
    fn tls_policy_matches_sslmode_semantics() {
        // No sslmode, disable, and prefer stay on the plaintext path the CI
        // service and dev container have always used.
        assert_eq!(tls_policy("postgres://u:p@h:5432/db"), TlsPolicy::PlainText);
        assert_eq!(
            tls_policy("postgres://u:p@h:5432/db?sslmode=disable"),
            TlsPolicy::PlainText
        );
        assert_eq!(
            tls_policy("postgres://u:p@h:5432/db?sslmode=prefer"),
            TlsPolicy::PlainText
        );

        // require must be honored with verified TLS, never downgraded.
        assert_eq!(
            tls_policy("postgres://u:p@h:5432/db?sslmode=require"),
            TlsPolicy::Required
        );

        // Modes tokio-postgres cannot express are refused, not downgraded.
        assert_eq!(
            tls_policy("postgres://u:p@h:5432/db?sslmode=verify-ca"),
            TlsPolicy::UnsupportedMode("verify-ca or verify-full")
        );
        assert_eq!(
            tls_policy("postgres://u:p@h:5432/db?sslmode=verify-full"),
            TlsPolicy::UnsupportedMode("verify-ca or verify-full")
        );
        assert_eq!(
            tls_policy("postgres://u:p@h:5432/db?sslmode=nonsense"),
            TlsPolicy::UnsupportedMode("an unsupported sslmode")
        );
    }

    #[test]
    fn selection_policy_env_beats_dotenv_beats_fallback() {
        let env = Some("postgres://env:pw@envhost:1/envdb".to_string());
        let dotenv = Some("postgres://dotenv:pw@dotenvhost:2/dotenvdb".to_string());
        assert_eq!(
            configured_url_from_parts(env.clone(), dotenv.clone()),
            "postgres://env:pw@envhost:1/envdb"
        );

        let none: Option<String> = None;
        assert_eq!(
            configured_url_from_parts(none.clone(), dotenv),
            "postgres://dotenv:pw@dotenvhost:2/dotenvdb"
        );
        assert_eq!(
            configured_url_from_parts(none, None),
            FALLBACK_URL,
            "no env, no dotenv: CI service contract applies"
        );
    }

    #[test]
    fn selection_policy_treats_empty_strings_as_unset() {
        let empty = Some(String::new());
        assert_eq!(
            configured_url_from_parts(empty.clone(), Some("postgres://d:pw@h:2/d".to_string())),
            "postgres://d:pw@h:2/d"
        );
        assert_eq!(configured_url_from_parts(empty, None), FALLBACK_URL);
    }

    #[serial_test::serial]
    #[test]
    fn explicitly_empty_db_url_does_not_block_the_dotenv_value() {
        // The regression: an explicitly EMPTY DB_URL env var used to make
        // dotenv skip loading the file's DB_URL, so the .env value was lost
        // and selection fell to the CI fallback. The .env value must win
        // over the fallback even when the env var is set-but-empty.
        let dir = std::env::temp_dir().join("pwr-bot-db-selection-test");
        std::fs::create_dir_all(&dir).expect("create temp cwd");
        std::fs::write(
            dir.join(".env"),
            "DB_URL=postgres://dotenv:pw@dotenvhost:2/dotenvdb\n",
        )
        .expect("write .env fixture");

        let previous_dir = std::env::current_dir().expect("read current dir");
        std::env::set_current_dir(&dir).expect("enter temp cwd");
        unsafe { std::env::set_var("DB_URL", "") };

        let selected = effective_configured_url();

        unsafe { std::env::remove_var("DB_URL") };
        std::env::set_current_dir(previous_dir).expect("restore cwd");

        assert_eq!(
            selected, "postgres://dotenv:pw@dotenvhost:2/dotenvdb",
            "empty env var must not shadow the .env value"
        );
    }

    #[serial_test::serial]
    #[test]
    fn non_empty_db_url_still_beats_the_dotenv_file() {
        let dir = std::env::temp_dir().join("pwr-bot-db-selection-test");
        std::fs::create_dir_all(&dir).expect("create temp cwd");
        std::fs::write(
            dir.join(".env"),
            "DB_URL=postgres://dotenv:pw@dotenvhost:2/dotenvdb\n",
        )
        .expect("write .env fixture");

        let previous_dir = std::env::current_dir().expect("read current dir");
        std::env::set_current_dir(&dir).expect("enter temp cwd");
        unsafe { std::env::set_var("DB_URL", "postgres://env:pw@envhost:1/envdb") };

        let selected = effective_configured_url();

        unsafe { std::env::remove_var("DB_URL") };
        std::env::set_current_dir(previous_dir).expect("restore cwd");

        assert_eq!(selected, "postgres://env:pw@envhost:1/envdb");
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn unreachable_url_fails_the_probe() {
        // Port 9 (discard) is unassigned; nothing local answers TCP there.
        assert!(
            !probe_connectable("postgres://pwr_bot:pwr_bot@127.0.0.1:9/pwr_bot").await,
            "probe must fail against a closed port"
        );
    }
}
