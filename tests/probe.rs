//! Locates built plugin binaries for the integration test suites.
//!
//! `CARGO_BIN_EXE_<name>` is only set when the harness built the binary
//! itself (e.g. the root crate's own `arg_echo_plugin` fixture bin), so the
//! host-side suites probe the workspace build output under `target/{profile}`
//! like the per-suite copies this module replaces.

use std::path::PathBuf;

/// Locates the built binary named `bin_name`: the `CARGO_BIN_EXE_<name>`
/// fast path when the harness built it, otherwise the `debug` then `release`
/// profile under the cargo target dir.
pub fn probe_binary(bin_name: &str) -> PathBuf {
    let env_hint = match bin_name {
        "hello" => option_env!("CARGO_BIN_EXE_hello"),
        "settings" => option_env!("CARGO_BIN_EXE_settings"),
        "feed-settings" => option_env!("CARGO_BIN_EXE_feed-settings"),
        "voice-settings" => option_env!("CARGO_BIN_EXE_voice-settings"),
        "arg_echo_plugin" => option_env!("CARGO_BIN_EXE_arg_echo_plugin"),
        _ => None,
    };
    if let Some(path) = env_hint {
        return PathBuf::from(path);
    }
    let target = match option_env!("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"),
    };
    for profile in ["debug", "release"] {
        let candidate = target.join(profile).join(bin_name);
        if candidate.exists() {
            return candidate;
        }
    }
    let hint = match bin_name {
        "hello" => concat!(
            "test-plugin fixture not built; run `cargo build -p hello` ",
            "(or `cargo build --workspace`) first"
        ),
        "settings" => concat!(
            "settings not built; run `cargo build -p settings` ",
            "(or `cargo build --workspace`) first"
        ),
        "feed-settings" => concat!(
            "feed-settings not built; run `cargo build -p feed-settings` ",
            "(or `cargo build --workspace`) first"
        ),
        "voice-settings" => concat!(
            "voice-settings not built; run `cargo build -p voice-settings` ",
            "(or `cargo build --workspace`) first"
        ),
        "arg_echo_plugin" => concat!(
            "test-plugin fixture not built; run `cargo build -p pwr-bot ",
            "--bin arg_echo_plugin` first"
        ),
        _ => "fixture not built; run `cargo build --workspace` first",
    };
    panic!("{hint}");
}
