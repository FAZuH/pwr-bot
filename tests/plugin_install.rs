//! Integration tests for the external plugin install path: downloading a
//! binary from a pinned catalog entry, verifying its sha256, installing it
//! atomically, and spawning the installed binary through
//! [`PluginManager`]. Pure stdio — no database.
//!
//! The catalog pins are https-only in production
//! ([`PluginCatalog::load`](pwr_bot::plugin::PluginCatalog) rejects http
//! urls), so these tests serve the fixture over plain http on localhost via
//! httpmock and build their own client without `https_only`; production
//! uses the https-only [`download_client`](pwr_bot::plugin::install::download_client).

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use httpmock::Method::GET;
use httpmock::MockServer;
use pwr_bot::plugin::CatalogEntry;
use pwr_bot::plugin::InstallError;
use pwr_bot::plugin::PluginManager;
use pwr_bot::plugin::RespawnPolicy;
use pwr_bot::plugin::install;
use tempfile::tempdir;
use wreq::Client;

mod probe;
use probe::probe_binary;

/// A plain client for the local http mock. No `https_only`: the mock serves
/// plain http on localhost, which production's https-only
/// `download_client` would reject by design.
fn test_client() -> Client {
    Client::builder().build().expect("wreq client construction")
}

/// Pins `entry` to the sha256 of `bytes` and serves those bytes at
/// `url_path`.
fn pinned_entry(name: &str, url: &str, bytes: &[u8]) -> CatalogEntry {
    let dir = tempdir().expect("temp dir");
    let pin_path = dir.path().join("pin");
    std::fs::write(&pin_path, bytes).expect("write pin file");
    CatalogEntry {
        name: name.to_string(),
        url: url.to_string(),
        sha256: install::sha256_hex(&pin_path).expect("pin sha256"),
        manifest: catalog_manifest(name),
        auto_enable: false,
    }
}

/// A catalog manifest for `name`, mirroring the fixture's hello.
fn catalog_manifest(name: &str) -> pwr_plugin_protocol::Manifest {
    pwr_plugin_protocol::Manifest {
        name: name.to_string(),
        description: "Test plugin".into(),
        version: "0.1.0".into(),
        commands: vec![pwr_plugin_protocol::CommandDef {
            create_command: serde_json::json!({
                "name": name,
                "description": "Say hello from a plugin",
            }),
        }],
        event_handlers: vec![],
        tasks: vec![],
        api_version: pwr_plugin_protocol::API_VERSION,
    }
}

// ── end-to-end: download → verify → install → spawn ────────────────────────

#[tokio::test]
async fn install_verified_downloads_verifies_and_spawns() {
    let server = MockServer::start();
    let binary = std::fs::read(probe_binary("hello")).expect("read fixture binary");
    let mock = server.mock(|when, then| {
        when.method(GET).path("/hello_plugin");
        then.status(200).body(&binary);
    });

    let plugins_dir = tempdir().expect("plugins dir");
    let entry = pinned_entry("hello", &server.url("/hello_plugin"), &binary);
    let installed = install::install_verified(&test_client(), &entry, plugins_dir.path())
        .await
        .expect("install verified binary");
    mock.assert();

    let expected = plugins_dir.path().join("hello");
    assert_eq!(installed, expected);
    let mode = std::fs::metadata(&installed)
        .expect("installed binary metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o755, "installed binary must be executable");

    // The installed binary is ready for PluginManager::spawn.
    let manager = Arc::new(PluginManager::new(None, RespawnPolicy::default()));
    manager
        .spawn("hello", &installed, None, &[], &[])
        .await
        .expect("spawn installed binary");
    assert!(manager.is_running("hello").await);
    manager.unload("hello", &[]).await.expect("teardown");
    assert!(!manager.is_running("hello").await);
}

// ── tampered download: verify fails, nothing is installed ──────────────────

#[tokio::test]
async fn install_verified_rejects_tampered_bytes_and_installs_nothing() {
    let server = MockServer::start();
    let binary = std::fs::read(probe_binary("hello")).expect("read fixture binary");
    let mut tampered = binary.clone();
    tampered[0] ^= 0xff;
    server.mock(|when, then| {
        when.method(GET).path("/hello_plugin");
        then.status(200).body(&tampered);
    });

    let plugins_dir = tempdir().expect("plugins dir");
    // Pin is computed from the pristine bytes: the tampered download must
    // fail verification.
    let entry = pinned_entry("hello", &server.url("/hello_plugin"), &binary);
    let err = install::install_verified(&test_client(), &entry, plugins_dir.path())
        .await
        .expect_err("tampered bytes must not verify");

    assert!(matches!(err, InstallError::Verify { .. }));
    assert!(
        !plugins_dir.path().join("hello").exists(),
        "nothing may be installed on a verify failure"
    );
    let leftovers: Vec<_> = std::fs::read_dir(plugins_dir.path())
        .expect("read plugins dir")
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp file must be cleaned up on verify failure"
    );
}

// ── redirect policy: a non-https hop stops the redirect ────────────────────

#[tokio::test]
async fn redirect_policy_stops_on_a_non_https_hop() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/start");
        then.status(302).header("location", server.url("/target"));
    });
    let target = server.mock(|when, then| {
        when.method(GET).path("/target");
        then.status(200).body("ok");
    });

    // The policy only follows https hops; the mock's http target must stop
    // the redirect and leave the target untouched.
    let client = Client::builder()
        .redirect(install::redirect_policy())
        .build()
        .expect("wreq client construction");
    let resp = client
        .get(server.url("/start"))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 302, "redirect must stop, not follow");
    assert_eq!(target.hits(), 0, "http target must not be requested");
}
