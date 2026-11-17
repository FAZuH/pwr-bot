//! Build-time helper: gives diesel-linked plugin binaries a runpath to
//! their libpq, so they stay self-sufficient under the host's cleared spawn
//! environment (`env_clear` in `src/plugin/mod.rs` keeps only `PATH`).
//!
//! Each diesel-linked plugin crate (`feed`, `voice`, `welcome`) calls
//! [`emit_rpath`] from its `build.rs`. Discovery order: an explicit
//! `PQ_LIB_DIR` (per-target first, like pq-sys itself), then
//! `pg_config --libdir`, then a scan of `LIBRARY_PATH` for a directory
//! holding libpq. When nothing is found — libpq on the default loader path,
//! as on CI — nothing is emitted and the link proceeds as before.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

/// Discovers the libpq directories and emits one
/// `cargo:rustc-link-arg=-Wl,-rpath,<dir>` per directory. Called from a
/// plugin crate's `build.rs`, where these directives apply to that crate's
/// own binary link.
pub fn emit_rpath() {
    println!("cargo:rerun-if-env-changed=PQ_LIB_DIR");
    println!("cargo:rerun-if-env-changed=LIBRARY_PATH");
    println!("cargo:rerun-if-env-changed=PATH");
    if let Ok(target) = std::env::var("TARGET") {
        println!(
            "cargo:rerun-if-env-changed=PQ_LIB_DIR_{}",
            target.to_ascii_uppercase().replace('-', "_")
        );
    }
    let env: HashMap<String, String> = std::env::vars().collect();
    for dir in discover(&env, &pg_config_libdir(), &dir_holds_libpq) {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
    }
}

/// The libpq directories for this build, in priority order and deduplicated:
/// explicit `PQ_LIB_DIR` first, then `pg_config`, then the `LIBRARY_PATH`
/// scan. Pure over its inputs so tests can pin the precedence.
pub fn discover(
    env: &HashMap<String, String>,
    pg_config_dir: &Option<PathBuf>,
    dir_holds_libpq: &dyn Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut push = |dir: PathBuf| {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    };
    // An explicit lib dir wins: per-target first, like pq-sys itself.
    if let Some(target) = env.get("TARGET") {
        let key = format!(
            "PQ_LIB_DIR_{}",
            target.to_ascii_uppercase().replace('-', "_")
        );
        if let Some(dir) = env.get(&key) {
            push(PathBuf::from(dir));
        }
    }
    if let Some(dir) = env.get("PQ_LIB_DIR") {
        push(PathBuf::from(dir));
    }
    if let Some(dir) = pg_config_dir {
        push(dir.clone());
    }
    // Otherwise scan the linker's own search list for a dir holding libpq.
    if let Some(library_path) = env.get("LIBRARY_PATH") {
        for dir in std::env::split_paths(library_path) {
            if dir_holds_libpq(&dir) {
                push(dir);
            }
        }
    }
    dirs
}

/// The `pg_config --libdir` directory, when a postgres install is on
/// `PATH`. `None` otherwise; failures are silent because the other sources
/// may still know the answer.
fn pg_config_libdir() -> Option<PathBuf> {
    let output = Command::new("pg_config").arg("--libdir").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let dir = String::from_utf8(output.stdout).ok()?;
    let dir = dir.trim();
    if dir.is_empty() {
        return None;
    }
    Some(PathBuf::from(dir))
}

/// Whether `dir` holds a libpq shared object.
fn dir_holds_libpq(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("libpq.so"))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn never(_: &Path) -> bool {
        false
    }

    #[test]
    fn an_explicit_lib_dir_is_used() {
        assert_eq!(
            discover(&env(&[("PQ_LIB_DIR", "/opt/pq/lib")]), &None, &never),
            vec![PathBuf::from("/opt/pq/lib")]
        );
    }

    #[test]
    fn the_per_target_lib_dir_comes_first() {
        assert_eq!(
            discover(
                &env(&[
                    ("TARGET", "x86_64-unknown-linux-gnu"),
                    ("PQ_LIB_DIR_X86_64_UNKNOWN_LINUX_GNU", "/opt/pq/target/lib"),
                    ("PQ_LIB_DIR", "/opt/pq/lib"),
                ]),
                &None,
                &never
            ),
            vec![
                PathBuf::from("/opt/pq/target/lib"),
                PathBuf::from("/opt/pq/lib"),
            ]
        );
    }

    #[test]
    fn the_pg_config_dir_is_used() {
        assert_eq!(
            discover(
                &env(&[]),
                &Some(PathBuf::from("/usr/lib/postgresql/18/lib")),
                &never
            ),
            vec![PathBuf::from("/usr/lib/postgresql/18/lib")]
        );
    }

    #[test]
    fn the_library_path_scan_picks_the_dir_holding_libpq() {
        let holds = |dir: &Path| dir == Path::new("/nix/store/libpq/lib");
        assert_eq!(
            discover(
                &env(&[("LIBRARY_PATH", "/nope:/nix/store/libpq/lib")]),
                &None,
                &holds
            ),
            vec![PathBuf::from("/nix/store/libpq/lib")]
        );
    }

    #[test]
    fn each_dir_is_listed_once() {
        let holds = |dir: &Path| dir == Path::new("/opt/pq/lib");
        assert_eq!(
            discover(
                &env(&[
                    ("PQ_LIB_DIR", "/opt/pq/lib"),
                    ("LIBRARY_PATH", "/opt/pq/lib"),
                ]),
                &Some(PathBuf::from("/opt/pq/lib")),
                &holds
            ),
            vec![PathBuf::from("/opt/pq/lib")]
        );
    }

    #[test]
    fn nothing_is_emitted_when_libpq_needs_no_help() {
        assert_eq!(discover(&env(&[]), &None, &never), Vec::<PathBuf>::new());
    }
}
