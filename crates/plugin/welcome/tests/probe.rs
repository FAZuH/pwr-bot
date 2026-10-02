//! Locates built plugin binaries for the integration test suites.
//!
//! `CARGO_BIN_EXE_<name>` is set by the harness for this package's own bins,
//! so the fast path normally hits. Otherwise the workspace build output under
//! `target/{profile}` is probed, like the host-side copy of this module does.

use std::path::PathBuf;

/// The workspace root: three levels up from this crate (`crates/plugin/welcome`).
const WORKSPACE_ROOT: &str = "../../..";

/// Locates the built binary named `bin_name`: the `CARGO_BIN_EXE_<name>`
/// fast path when the harness built it, otherwise the `debug` then `release`
/// profile under the cargo target dir.
pub fn probe_binary(bin_name: &str) -> PathBuf {
    if let Ok(path) = std::env::var(format!("CARGO_BIN_EXE_{bin_name}")) {
        return PathBuf::from(path);
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(WORKSPACE_ROOT);
    let target = match option_env!("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => root.join("target"),
    };
    // `cargo test` does not rebuild plain bin artifacts for sibling
    // packages: build the fixture so fresh checkouts have it and edited
    // fixtures never run stale.
    let mut cmd = std::process::Command::new("cargo");
    cmd.args(["build", "-j", "2", "-p", bin_name]);
    cmd.current_dir(&root);
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
    panic!("fixture `{bin_name}` missing after cargo build");
}
