//! Locating the plugin binary to spawn.
//!
//! This tool is itself a cargo-built binary, so it cannot rely on
//! `CARGO_BIN_EXE_*` (only integration tests of the same package get those).
//! Named plugin binaries are therefore located in the cargo build layout
//! this executable lives in — the same layout `tests/probe.rs` probes for
//! the integration suites.

use std::path::Path;
use std::path::PathBuf;

/// Which plugin to spawn: a bare binary name or an explicit path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginSpec {
    /// A bare binary name (e.g. `hello`), resolved against the cargo build
    /// profile directories ([`resolve`]).
    Name(String),
    /// An explicit path, used verbatim.
    Path(PathBuf),
}

impl PluginSpec {
    /// Classifies a `--plugin` flag value: a value containing a path
    /// separator is an explicit path, anything else a bare binary name.
    pub fn from_flag(value: &str) -> PluginSpec {
        if value.contains(std::path::is_separator) {
            PluginSpec::Path(PathBuf::from(value))
        } else {
            PluginSpec::Name(value.to_owned())
        }
    }
}

impl std::fmt::Display for PluginSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginSpec::Name(name) => f.write_str(name),
            PluginSpec::Path(path) => write!(f, "{}", path.display()),
        }
    }
}

/// The cargo build profile directories probed for named plugin binaries,
/// mirroring `tests/probe.rs`.
const PROFILE_DIRS: [&str; 2] = ["debug", "release"];

/// Every path a named plugin binary could occupy, in preference order:
/// the directory of this executable first (the same `target/<profile>/` the
/// preview tool itself was built into), then each profile directory under
/// the same `target/` — deduplicated, `debug` before `release`.
pub fn candidate_paths(exe: &Path, name: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    // A bare relative executable name has no parent directory to resolve
    // against; there is nothing to probe.
    let Some(profile_dir) = exe.parent().filter(|dir| !dir.as_os_str().is_empty()) else {
        return candidates;
    };
    candidates.push(profile_dir.join(name));
    if let Some(target_dir) = profile_dir.parent() {
        for profile in PROFILE_DIRS {
            let candidate = target_dir.join(profile).join(name);
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

/// Resolves which binary to spawn for `spec`, given this tool's own path:
///
/// 1. an explicit path is used verbatim (it must exist);
/// 2. a bare name is looked up in [`candidate_paths`] order — sibling of
///    this executable first, then the sibling profile directories.
///
/// The error lists every path tried plus the `cargo build` that produces
/// them.
pub fn resolve(exe: &Path, spec: &PluginSpec) -> anyhow::Result<PathBuf> {
    match spec {
        PluginSpec::Path(path) => {
            if path.is_file() {
                Ok(path.clone())
            } else {
                anyhow::bail!("`{}` does not exist", path.display());
            }
        }
        PluginSpec::Name(name) => {
            let candidates = candidate_paths(exe, name);
            let found = candidates.iter().find(|candidate| candidate.is_file());
            match found {
                Some(path) => Ok(path.clone()),
                None => anyhow::bail!(
                    "no built `{name}` binary; tried:\n{}\nbuild it with `cargo build -p {name}`",
                    candidates
                        .iter()
                        .map(|candidate| format!("  {}", candidate.display()))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_flag_takes_plain_names_as_names() {
        assert_eq!(
            PluginSpec::from_flag("hello"),
            PluginSpec::Name("hello".into())
        );
        assert_eq!(
            PluginSpec::from_flag("settings"),
            PluginSpec::Name("settings".into())
        );
    }

    #[test]
    fn from_flag_takes_separator_bearing_values_as_paths() {
        assert_eq!(
            PluginSpec::from_flag("./target/debug/hello"),
            PluginSpec::Path(PathBuf::from("./target/debug/hello"))
        );
        assert_eq!(
            PluginSpec::from_flag("/usr/local/bin/hello"),
            PluginSpec::Path(PathBuf::from("/usr/local/bin/hello"))
        );
    }

    #[test]
    fn candidate_paths_prefer_the_executables_own_profile_dir() {
        let exe = Path::new("/repo/target/debug/pwr-preview");
        assert_eq!(
            candidate_paths(exe, "hello"),
            vec![
                PathBuf::from("/repo/target/debug/hello"),
                PathBuf::from("/repo/target/release/hello"),
            ]
        );
    }

    #[test]
    fn candidate_paths_fall_back_across_profiles_from_custom_profiles() {
        let exe = Path::new("/repo/target/myprofile/pwr-preview");
        assert_eq!(
            candidate_paths(exe, "hello"),
            vec![
                PathBuf::from("/repo/target/myprofile/hello"),
                PathBuf::from("/repo/target/debug/hello"),
                PathBuf::from("/repo/target/release/hello"),
            ]
        );
    }

    #[test]
    fn candidate_paths_are_empty_when_the_exe_has_no_parent_dir() {
        assert!(candidate_paths(Path::new("pwr-preview"), "hello").is_empty());
    }

    #[test]
    fn resolve_uses_an_existing_explicit_path_verbatim() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plugin = dir.path().join("custom-plugin");
        std::fs::write(&plugin, b"#!/bin/sh\n").expect("write fake plugin");
        let exe = dir.path().join("pwr-preview");
        let spec = PluginSpec::Path(plugin.clone());
        assert_eq!(resolve(&exe, &spec).expect("resolve"), plugin);
    }

    #[test]
    fn resolve_rejects_a_missing_explicit_path() {
        let exe = Path::new("/repo/target/debug/pwr-preview");
        let spec = PluginSpec::Path(PathBuf::from("/nope/hello"));
        let error = resolve(exe, &spec).expect_err("missing path must fail");
        assert!(error.to_string().contains("/nope/hello"), "got: {error}");
    }

    #[test]
    fn resolve_falls_back_to_the_other_profile_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("target");
        let exe = target.join("debug").join("pwr-preview");
        std::fs::create_dir_all(exe.parent().expect("exe parent")).expect("create debug dir");
        // The plugin is only built in release: the sibling-profile fallback
        // must find it.
        let release_plugin = target.join("release").join("hello");
        std::fs::create_dir_all(release_plugin.parent().expect("plugin parent"))
            .expect("create release dir");
        std::fs::write(&release_plugin, b"#!/bin/sh\n").expect("write fake plugin");
        let spec = PluginSpec::Name("hello".into());
        assert_eq!(resolve(&exe, &spec).expect("resolve"), release_plugin);
    }

    #[test]
    fn resolve_errors_listing_every_tried_path_and_the_build_hint() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("target");
        let exe = target.join("debug").join("pwr-preview");
        let spec = PluginSpec::Name("hello".into());
        let error = resolve(&exe, &spec).expect_err("unbuilt plugin must fail");
        let message = error.to_string();
        assert!(
            message.contains("no built `hello` binary"),
            "got: {message}"
        );
        assert!(message.contains("/target/debug/hello"), "got: {message}");
        assert!(message.contains("/target/release/hello"), "got: {message}");
        assert!(message.contains("cargo build -p hello"), "got: {message}");
    }
}
