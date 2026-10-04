//! Every migration source in the workspace owns a globally unique version.
//!
//! The ledger is one shared table (`__diesel_schema_migrations`) written by
//! every source, so two sources sharing a version string is a silent
//! data-loss bug: the second is treated as already applied and never creates
//! its tables. No single crate can catch that alone, so the check lives here
//! and discovers sources by workspace layout rather than by name.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

/// The deepest directory nesting below the workspace root a migration source
/// may sit at: `migrations` (0), `crates/*/migrations` (1),
/// `crates/*/*/migrations` (2).
const MAX_SOURCE_DEPTH: usize = 2;

/// Every migration source directory under `root`, sorted.
fn migration_sources(root: &Path) -> Vec<PathBuf> {
    let mut found: BTreeSet<PathBuf> = BTreeSet::new();
    collect_sources(root, 0, &mut found);
    found.into_iter().collect()
}

fn collect_sources(dir: &Path, depth: usize, found: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().is_some_and(|name| name == "migrations") {
            found.insert(path);
        } else if depth <= MAX_SOURCE_DEPTH {
            collect_sources(&path, depth + 1, found);
        }
    }
}

/// The ledger version a migration directory name carries: its leading
/// timestamp with the dashes the directory name uses removed. Diesel records
/// exactly these digits in `__diesel_schema_migrations.version`.
fn ledger_version(directory_name: &str) -> String {
    directory_name
        .split('_')
        .next()
        .unwrap_or_default()
        .replace('-', "")
}

/// The versions each source declares, keyed by source path, sorted by version.
fn versions_by_source(root: &Path) -> BTreeMap<PathBuf, Vec<String>> {
    migration_sources(root)
        .into_iter()
        .map(|source| {
            let mut versions: Vec<String> = std::fs::read_dir(&source)
                .expect("a discovered migration source is readable")
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .map(|entry| ledger_version(&entry.file_name().to_string_lossy()))
                .collect();
            versions.sort();
            (source, versions)
        })
        .collect()
}

/// Two sources sharing a version would silently lose the second source's
/// tables: the shared ledger already records the version, so the second run
/// applies nothing.
#[test]
fn every_migration_source_owns_a_globally_unique_version() {
    let by_source = versions_by_source(Path::new(env!("CARGO_MANIFEST_DIR")));
    assert!(
        by_source.len() >= 2,
        "the workspace declares more than one migration source, so uniqueness is a real property: {by_source:?}"
    );

    let claimed: Vec<(&Path, &String)> = by_source
        .values()
        .flatten()
        .map(|version| (Path::new("<source>"), version))
        .collect();
    let unique: BTreeSet<&String> = claimed.iter().map(|(_, version)| *version).collect();
    assert_eq!(
        unique.len(),
        claimed.len(),
        "two migration sources share a version, so the second never creates its tables: {by_source:?}"
    );
}

/// Each source is a single migration, so "one version per source" and "one
/// source per version" are the same statement. Pinned apart from the check
/// above so a future second migration in one source is a deliberate, visible
/// change rather than an accident.
#[test]
fn each_migration_source_declares_exactly_one_migration() {
    let by_source = versions_by_source(Path::new(env!("CARGO_MANIFEST_DIR")));
    let multiple: Vec<(&Path, usize)> = by_source
        .iter()
        .filter(|(_, versions)| versions.len() != 1)
        .map(|(source, versions)| (source.as_path(), versions.len()))
        .collect();
    assert!(
        multiple.is_empty(),
        "every source declares exactly one migration: {multiple:?}"
    );
}
