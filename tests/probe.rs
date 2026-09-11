//! Locates built plugin binaries for the integration test suites.
//!
//! `CARGO_BIN_EXE_<name>` is only set when the harness built the binary
//! itself (e.g. the root crate's own `arg-echo-plugin` fixture bin), so the
//! host-side suites probe the workspace build output under `target/{profile}`
//! like the per-suite copies this module replaces.

use std::path::PathBuf;

/// Locates the built binary named `bin_name`: the `CARGO_BIN_EXE_<name>`
/// fast path when the harness built it, otherwise the `debug` then `release`
/// profile under the cargo target dir.
pub fn probe_binary(bin_name: &str) -> PathBuf {
    if let Ok(path) = std::env::var(format!("CARGO_BIN_EXE_{bin_name}")) {
        return PathBuf::from(path);
    }
    let target = match option_env!("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"),
    };
    // `cargo test` does not produce plain bin artifacts for sibling
    // packages, so build the missing fixture on demand.
    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("build").current_dir(env!("CARGO_MANIFEST_DIR"));
    if bin_name == "arg-echo-plugin" {
        cmd.args(["--bin", "arg-echo-plugin"]);
    } else {
        cmd.args(["-p", bin_name]);
    }
    assert!(
        cmd.status().is_ok_and(|s| s.success()),
        "cargo build failed for fixture `{bin_name}`"
    );
    for profile in ["debug", "release"] {
        let candidate = target.join(profile).join(bin_name);
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("fixture `{bin_name}` still missing after cargo build");
}
