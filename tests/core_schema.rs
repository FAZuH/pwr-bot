//! The core migration source owns exactly the host's own tables.
//!
//! This assertion is deliberately generic: it names the four core tables and
//! nothing else, so it does not enumerate what any plugin owns. A plugin's
//! migration source is asserted in that plugin's own crate.

/// The tables the host's core storage owns (ADR-0015).
const CORE_TABLES: &[&str] = &["server_settings", "bot_meta", "plugin_kv", "guild_plugins"];

/// Every `CREATE TABLE IF NOT EXISTS <name>` in `sql`, in source order.
fn declared_tables(sql: &str) -> Vec<String> {
    sql.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("CREATE TABLE IF NOT EXISTS ")?;
            let name = rest.split(['(', ' ', ';']).next()?;
            Some(name.to_string())
        })
        .collect()
}

#[test]
fn core_migrations_claim_only_core_storage() {
    let core = include_str!("../migrations/20260925-000000-0000_core_storage/up.sql");
    let historical_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("migrations/2026-04-29-122252-0000_initial_schema");

    assert!(!historical_path.exists());
    assert_eq!(
        declared_tables(core),
        CORE_TABLES,
        "the core migration declares exactly the host's own tables"
    );
    assert!(
        core.starts_with(
            "-- Each migration source owns only the tables in its up.sql and uses CREATE TABLE IF NOT EXISTS."
        ),
        "the migration source carries its ownership header"
    );
}
