use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;

use poise::serenity_prelude::Http;
use poise::serenity_prelude::Token;

use pwr_bot::bot::Data;
use pwr_bot::bot::host_ctx::PoiseHostCtx;
use pwr_bot::bot::plugin::host_registry;
use pwr_bot::bot::plugin::loader;
use pwr_bot::bot::plugin::loader::LoadedPlugin;
use pwr_bot::bot::plugin::registry::PluginRegistry;
use pwr_bot::config::{Config, Features};
use pwr_bot::event::event_bus::EventBus;
use pwr_bot::service::Services;

use super::noop_services::{NoopInternalOps, NoopSettingsProvider};

static BUILD_PLUGIN: OnceLock<()> = OnceLock::new();

/// Builds the test plugin `.so` once per test run.
pub fn build_test_plugin() {
    BUILD_PLUGIN.get_or_init(|| {
        let output = std::process::Command::new("cargo")
            .args(["build", "-p", "pwr-bot-test-plugin"])
            .output()
            .expect("failed to run cargo build for test plugin");
        assert!(
            output.status.success(),
            "test plugin build failed:\n{}",
            String::from_utf8_lossy(&output.stderr),
        );
    });
}

/// Returns the path to the built test plugin `.so`.
pub fn test_plugin_path() -> PathBuf {
    let exe = std::env::current_exe().expect("failed to get test binary path");
    // Test binary: target/debug/deps/plugin_ffi-<hash>
    // Plugin .so:  target/debug/libpwr_bot_test_plugin.so
    let deps_dir = exe.parent().expect("test binary has no parent");
    let build_dir = deps_dir.parent().expect("deps dir has no parent");
    build_dir.join("libpwr_bot_test_plugin.so")
}

/// Loads the test plugin from disk.
///
/// # Safety
///
/// The `.so` is built from the `pwr-bot-test-plugin` crate which is trusted.
pub unsafe fn load_test_plugin() -> LoadedPlugin {
    build_test_plugin();
    let path = test_plugin_path();
    unsafe { loader::load_plugin(&path).expect("failed to load test plugin") }
}

/// Creates a test `Config` with known values.
pub fn test_config() -> Config {
    let mut flags = HashMap::new();
    flags.insert("test_feature".to_string(), true);
    flags.insert("other_feature".to_string(), false);

    Config {
        poll_interval: std::time::Duration::from_secs(42),
        db_url: "postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot".into(),
        discord_token: "fake-token".into(),
        discord_application_id: Some(12345),
        admin_id: "12345".into(),
        data_path: std::env::temp_dir().join("pwr-bot-test-data"),
        logs_path: std::env::temp_dir().join("pwr-bot-test-logs"),
        plugin_dir: std::env::temp_dir().join("pwr-bot-test-plugins"),
        features: Features::new(flags),
        version: "0.1.0-test".into(),
    }
}

/// Creates a test `Data` with stubs for services.
pub fn test_data() -> Arc<Data> {
    let config = Arc::new(test_config());
    let event_bus = Arc::new(EventBus::new());
    let plugin_registry = Arc::new(PluginRegistry::new());
    let services = Arc::new(Services {
        settings: Arc::new(NoopSettingsProvider),
        internal: Arc::new(NoopInternalOps),
    });

    Arc::new(Data {
        config,
        service: services,
        event_bus,
        plugin_registry,
        start_time: Instant::now(),
    })
}

/// Creates a fake HTTP client (no real Discord calls).
pub fn fake_http() -> Arc<Http> {
    // Token must have 3 non-empty dot-separated parts (format validation only).
    let token = Token::from_str("a.b.c").expect("invalid token");
    Arc::new(Http::new(token))
}

/// Sets up a system context for FFI dispatch paths.
pub fn setup_system_ctx(data: Arc<Data>) -> Arc<PoiseHostCtx> {
    let http = fake_http();
    let ctx = PoiseHostCtx::new_system(data, http);
    host_registry::set_system_ctx(ctx.clone());
    ctx
}

/// Resets all global `host_registry` state for test isolation.
pub fn reset_host_registry() {
    host_registry::reset_for_test();
}
