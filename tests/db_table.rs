//! Integration tests for database table operations.

use diesel::sql_types::BigInt;
use diesel::sql_types::Jsonb;
use diesel_async::RunQueryDsl;
use pwr_bot::entity::DbU64;
use pwr_bot::repo::traits::*;

mod common;

// --- 1. Test Harness Macro ---
// Handles setup, execution, and teardown automatically.
macro_rules! db_test {
    ($name:ident, |$db:ident| $body:block) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() {
            let $db = common::setup_db().await;

            // Execute the test logic
            $body

            common::teardown_db(&$db).await;
        }
    };
}

// --- 3. Refactored Tests ---

// The legacy per-guild settings rows, the one-time import source every
// plugin copies from on a guild's first read (ADR-0015).
//
// The host types no payload for this table — `ServerSettingsEntity` and the
// `Json<T>` newtype are gone — so these tests go through the same raw SQL the
// plugins use. What they pin is the contract that table has to keep: the
// column names and the `settings` JSON shape those queries index into, and
// the `delete_all` path the test harness relies on.
mod server_settings_table_tests {

    use super::*;

    // A plugin-visible snapshot, in the shape `ServerSettings` serialized.
    fn legacy_settings() -> serde_json::Value {
        serde_json::json!({
            "feeds": {"enabled": true, "channel_id": "123456789"},
            "voice": {"enabled": false},
            "welcome": {"enabled": true, "template_id": "2"}
        })
    }

    async fn insert_row(pool: &pwr_bot::repo::DbPool, guild_id: u64, settings: &serde_json::Value) {
        let mut conn = pool.get().await.expect("connect for insert");
        diesel::sql_query(
            "INSERT INTO server_settings (guild_id, settings) VALUES ($1, $2::text::jsonb)",
        )
        .bind::<BigInt, _>(guild_id as i64)
        .bind::<Jsonb, _>(settings)
        .execute(&mut conn)
        .await
        .expect("insert a legacy settings row");
    }

    // The columns and types the plugins' `sql_query` projections bind to.
    // A rename or retype here breaks every plugin's first read, silently.
    db_test!(the_legacy_row_shape_is_what_plugins_query, |db| {
        insert_row(&db.pool().clone(), 42, &legacy_settings()).await;

        let mut conn = db.pool().get().await.expect("connect for select");
        let row =
            diesel::sql_query("SELECT guild_id, settings FROM server_settings WHERE guild_id = $1")
                .bind::<BigInt, _>(42_i64)
                .get_result::<LegacySettingsRow>(&mut conn)
                .await
                .expect("a plugin-visible legacy settings row");

        assert_eq!(
            row.guild_id, 42,
            "guild_id is a BIGINT the plugins bind as i64"
        );
        assert_eq!(
            row.settings["feeds"]["channel_id"],
            serde_json::json!("123456789"),
            "the settings column indexes by guild then section, as the plugins' `#>>` paths assume"
        );
    });

    // The per-section paths the plugins actually read.
    db_test!(
        each_plugin_section_is_reachable_through_the_json_path,
        |db| {
            insert_row(&db.pool().clone(), 43, &legacy_settings()).await;

            let mut conn = db.pool().get().await.expect("connect for sections");
            let rows = diesel::sql_query(concat!(
                "SELECT (settings #>> '{feeds,enabled}')::boolean AS feeds_enabled, ",
                "(settings #>> '{voice,enabled}')::boolean AS voice_enabled, ",
                "(settings #>> '{welcome,template_id}') AS welcome_template ",
                "FROM server_settings WHERE guild_id = $1"
            ))
            .bind::<diesel::sql_types::BigInt, _>(43_i64)
            .load::<LegacySectionRow>(&mut conn)
            .await
            .expect("read each plugin's section");

            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].feeds_enabled, Some(true));
            assert_eq!(rows[0].voice_enabled, Some(false));
            assert_eq!(rows[0].welcome_template.as_deref(), Some("2"));
        }
    );

    // A guild with no legacy row reads as absent, which is what makes every
    // plugin's import fall back to its own defaults.
    db_test!(an_absent_guild_reads_as_no_row, |db| {
        let mut conn = db.pool().get().await.expect("connect for absence");
        let exists: bool = diesel::sql_query(
            "SELECT EXISTS (SELECT 1 FROM server_settings WHERE guild_id = $1) AS present",
        )
        .bind::<BigInt, _>(99_i64)
        .get_result::<ExistsRow>(&mut conn)
        .await
        .expect("query for an absent guild")
        .present;
        assert!(!exists, "a guild that never had legacy settings has no row");
    });

    // Upsert: a deployed instance re-running anything that touches the table
    // must not collide on the primary key.
    db_test!(a_second_row_for_the_same_guild_is_rejected, |db| {
        insert_row(&db.pool().clone(), 44, &legacy_settings()).await;

        let mut conn = db.pool().get().await.expect("connect for duplicate");
        let result = diesel::sql_query(
            "INSERT INTO server_settings (guild_id, settings) VALUES ($1, $2::text::jsonb)",
        )
        .bind::<BigInt, _>(44_i64)
        .bind::<Jsonb, _>(&legacy_settings())
        .execute(&mut conn)
        .await;

        assert!(
            result.is_err(),
            "guild_id is the primary key: one row per guild"
        );
    });

    // The harness cleans the table between tests through
    // `delete_all_tables`; if that path stopped clearing it, one test's
    // legacy row would leak into the next and every plugin's import would
    // see phantom data.
    db_test!(delete_all_clears_the_legacy_rows, |db| {
        insert_row(&db.pool().clone(), 45, &legacy_settings()).await;
        db.server_settings
            .delete_all()
            .await
            .expect("clear legacy rows");

        let mut conn = db.pool().get().await.expect("connect after clear");
        let remaining: i64 = diesel::sql_query("SELECT count(*) AS count FROM server_settings")
            .get_result::<CountRow>(&mut conn)
            .await
            .expect("count remaining legacy rows")
            .count;
        assert_eq!(remaining, 0, "delete_all empties the legacy table");
    });

    // The projection the plugins bind, mirroring their `QueryableByName` row.
    #[derive(diesel::QueryableByName)]
    struct LegacySettingsRow {
        #[diesel(sql_type = BigInt)]
        guild_id: i64,
        #[diesel(sql_type = Jsonb)]
        settings: serde_json::Value,
    }

    // A bare `SELECT EXISTS` needs a named column to bind to a row.
    #[derive(diesel::QueryableByName)]
    struct ExistsRow {
        #[diesel(sql_type = diesel::sql_types::Bool)]
        present: bool,
    }

    // Likewise for a bare aggregate.
    #[derive(diesel::QueryableByName)]
    struct CountRow {
        #[diesel(sql_type = BigInt)]
        count: i64,
    }

    // The per-section paths the plugins actually read, as separate columns so
    // each is bound the way a plugin's projection binds it.
    #[derive(diesel::QueryableByName)]
    struct LegacySectionRow {
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Bool>)]
        feeds_enabled: Option<bool>,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Bool>)]
        voice_enabled: Option<bool>,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        welcome_template: Option<String>,
    }
}

mod plugin_kv_table_tests {
    use super::*;

    db_test!(set_then_get_returns_value, |db| {
        db.plugin_kv
            .set("settings", "theme", "dark")
            .await
            .expect("Failed to set kv");

        let value = db
            .plugin_kv
            .get("settings", "theme")
            .await
            .unwrap()
            .expect("Key should exist after set");
        assert_eq!(value, "dark");
    });

    db_test!(get_absent_key_returns_none, |db| {
        let value = db.plugin_kv.get("settings", "nope").await.unwrap();
        assert!(value.is_none());
    });

    db_test!(set_upserts_same_key, |db| {
        db.plugin_kv.set("settings", "theme", "dark").await.unwrap();
        db.plugin_kv
            .set("settings", "theme", "light")
            .await
            .unwrap();

        let value = db
            .plugin_kv
            .get("settings", "theme")
            .await
            .unwrap()
            .expect("Key should exist after upsert");
        assert_eq!(value, "light");
    });

    db_test!(delete_then_get_none, |db| {
        db.plugin_kv.set("settings", "theme", "dark").await.unwrap();
        db.plugin_kv
            .delete("settings", "theme")
            .await
            .expect("Failed to delete kv");

        let value = db.plugin_kv.get("settings", "theme").await.unwrap();
        assert!(value.is_none());
    });

    db_test!(namespaces_are_isolated, |db| {
        db.plugin_kv.set("settings", "theme", "dark").await.unwrap();

        let value = db.plugin_kv.get("other", "theme").await.unwrap();
        assert!(
            value.is_none(),
            "Same key in another namespace must not leak"
        );
    });
}

mod guild_plugins_table_tests {
    use super::*;

    db_test!(set_enabled_then_list, |db| {
        db.guild_plugins
            .set_enabled(123, "hello", true)
            .await
            .expect("Failed to set plugin state");

        let rows = db
            .guild_plugins
            .list_for_guild(123)
            .await
            .expect("Failed to list guild plugins");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].guild_id, DbU64::from(123));
        assert_eq!(rows[0].plugin_name, "hello");
        assert!(rows[0].enabled);
    });

    db_test!(set_enabled_updates_existing, |db| {
        db.guild_plugins
            .set_enabled(123, "hello", true)
            .await
            .unwrap();
        db.guild_plugins
            .set_enabled(123, "hello", false)
            .await
            .unwrap();

        let rows = db.guild_plugins.list_for_guild(123).await.unwrap();
        assert_eq!(rows.len(), 1, "Upsert must not duplicate the row");
        assert!(!rows[0].enabled);
    });

    db_test!(delete_removes_plugin_state, |db| {
        db.guild_plugins
            .set_enabled(123, "hello", true)
            .await
            .unwrap();
        db.guild_plugins
            .delete(123, "hello")
            .await
            .expect("Failed to delete plugin state");

        let rows = db.guild_plugins.list_for_guild(123).await.unwrap();
        assert!(rows.is_empty());
    });

    db_test!(list_empty_for_unknown_guild, |db| {
        let rows = db
            .guild_plugins
            .list_for_guild(999)
            .await
            .expect("Failed to list guild plugins");
        assert!(rows.is_empty());
    });

    db_test!(guilds_are_isolated, |db| {
        db.guild_plugins
            .set_enabled(123, "hello", true)
            .await
            .unwrap();

        let rows = db.guild_plugins.list_for_guild(456).await.unwrap();
        assert!(rows.is_empty(), "Plugin state must not leak across guilds");
    });

    // A core plugin's per-guild state lives in the same table as a catalog
    // plugin's: `/plugin disable feed` persists `enabled = false` and the next
    // read has to hand that row back, or the core plugin re-enables itself on
    // the next start or guild join.
    db_test!(a_core_plugins_state_round_trips_through_the_table, |db| {
        db.guild_plugins
            .set_enabled(123, "feed", false)
            .await
            .expect("disable a core plugin");

        let rows = db.guild_plugins.list_for_guild(123).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].plugin_name, "feed");
        assert!(
            !rows[0].enabled,
            "the disabled core plugin reads back disabled"
        );

        db.guild_plugins
            .set_enabled(123, "feed", true)
            .await
            .expect("enable a core plugin");

        let rows = db.guild_plugins.list_for_guild(123).await.unwrap();
        assert_eq!(rows.len(), 1, "re-enabling upserts the same row");
        assert!(rows[0].enabled);
    });
}
