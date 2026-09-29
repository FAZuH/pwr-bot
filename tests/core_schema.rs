#[test]
fn voice_plugin_migration_claims_only_voice_storage() {
    let initial = include_str!(
        "../crates/plugin/voice/migrations/20260924-140100-0000_voice_owned_schema/up.sql"
    );
    let sentinel = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/plugin/voice/migrations/20260924-140000-0000_voice_storage");

    assert!(initial.contains("voice_sessions"));
    assert!(initial.contains("voice_settings"));
    assert!(!initial.contains("server_settings"));
    assert!(!sentinel.exists());
}

#[test]
fn feed_plugin_migration_claims_only_feed_storage() {
    let initial = include_str!(
        "../crates/plugin/feed/migrations/20260924-130100-0000_feed_owned_schema/up.sql"
    );
    let sentinel = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/plugin/feed/migrations/20260924-130000-0000_feed_settings");

    assert!(initial.contains("feeds"));
    assert!(initial.contains("feed_settings"));
    assert!(!initial.contains("server_settings"));
    assert!(!sentinel.exists());
}

#[test]
fn core_migrations_claim_only_core_storage() {
    let core = include_str!("../migrations/20260925-000000-0000_core_storage/up.sql");
    let historical_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("migrations/2026-04-29-122252-0000_initial_schema");

    assert!(!historical_path.exists());
    assert!(core.contains("server_settings"));
    assert!(core.contains("bot_meta"));
    assert!(core.contains("plugin_kv"));
    assert!(core.contains("guild_plugins"));
    assert!(!core.contains("voice_sessions"));
    assert!(!core.contains("feeds"));
    assert!(core.starts_with(
        "-- Each migration source owns only the tables in its up.sql and uses CREATE TABLE IF NOT EXISTS."
    ));
}
