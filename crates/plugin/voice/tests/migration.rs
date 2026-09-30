//! The voice plugin's own migration source owns only voice storage.
//!
//! Scoped to this crate: it reads this plugin's migration, so nothing here
//! names another plugin's tables or version.

/// This plugin's migration source, and the tables it must own.
const MIGRATION: &str =
    include_str!("../migrations/20260924-140100-0000_voice_owned_schema/up.sql");
const OWNED_TABLES: &[&str] = &[
    "voice_sessions",
    "voice_settings",
    "voice_settings_import_state",
];

/// The version this plugin's single migration carries. Uniqueness ACROSS
/// sources is a workspace property, checked once in the host's
/// `tests/migration_versions.rs`; what this crate owns is that the single
/// migration it declares is the one carrying this version.
const VERSION: &str = "202609241401000000";

/// Every table this migration declares, in source order. The match is
/// line-anchored so the ownership header's prose does not count.
fn declared_tables() -> Vec<String> {
    MIGRATION
        .lines()
        .filter_map(|line| {
            let rest = line
                .trim_start()
                .strip_prefix("CREATE TABLE IF NOT EXISTS ")?;
            rest.split(['(', ' ', ';']).next().map(str::to_string)
        })
        .collect()
}

#[test]
fn the_migration_creates_exactly_the_tables_this_plugin_owns() {
    assert_eq!(
        declared_tables(),
        OWNED_TABLES,
        "the migration declares exactly this plugin's tables, and no others"
    );
}

#[test]
fn the_migration_does_not_touch_the_hosts_legacy_table() {
    assert!(
        !MIGRATION.contains("server_settings"),
        "the legacy import source stays the host's; this plugin reads it, never writes it"
    );
}

#[test]
fn the_migration_source_is_idempotent() {
    for table in OWNED_TABLES {
        let create = format!("CREATE TABLE IF NOT EXISTS {table} (");
        assert!(
            MIGRATION.contains(&create),
            "every table is created with IF NOT EXISTS, so a repeated run applies nothing"
        );
    }
    assert!(
        MIGRATION.starts_with(
            "-- Each migration source owns only the tables in its up.sql and uses CREATE TABLE IF NOT EXISTS."
        ),
        "the migration source carries its ownership header"
    );
}

#[test]
fn the_declared_migration_carries_this_crates_version() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut entries: Vec<String> = std::fs::read_dir(&dir)
        .expect("migrations directory is readable")
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    entries.sort();

    assert_eq!(entries.len(), 1, "this plugin owns exactly one migration");
    // The directory name dashes the timestamp into segments; the ledger
    // version is those digits run together.
    assert_eq!(
        entries[0]
            .split('_')
            .next()
            .expect("migration entry carries a version prefix")
            .replace('-', ""),
        VERSION,
        "and it is the one this plugin declares"
    );
}

#[test]
fn the_retired_sentinel_migration_is_gone() {
    let sentinel = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("migrations/20260924-140000-0000_voice_storage");
    assert!(!sentinel.exists());
}
