//! Integration tests for database table operations.

use pwr_bot::entity::DbU64;
use pwr_bot::entity::FeedsSettings;
use pwr_bot::entity::Json;
use pwr_bot::entity::ServerSettingsEntity;
use pwr_bot::entity::WelcomeSettings;
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

mod server_settings_table_tests {
    use pwr_bot::entity::ServerSettings;
    use pwr_bot::entity::VoiceSettings;

    use super::*;

    fn create_settings(guild_id: u64, chan: &str) -> ServerSettingsEntity {
        ServerSettingsEntity {
            guild_id: DbU64::from(guild_id),
            settings: Json(ServerSettings {
                voice: VoiceSettings::default(),
                feeds: FeedsSettings {
                    enabled: Some(true),
                    channel_id: Some(chan.to_string()),
                    subscribe_role_id: None,
                    unsubscribe_role_id: None,
                },
                welcome: WelcomeSettings::default(),
            }),
        }
    }

    db_test!(insert_and_select, |db| {
        let id = db
            .server_settings
            .insert(&create_settings(123, "c1"))
            .await
            .unwrap();
        assert_eq!(id, 123);

        let fetched = db.server_settings.select(&123).await.unwrap().unwrap();
        assert_eq!(fetched.settings.0.feeds.channel_id, Some("c1".to_string()));
    });

    db_test!(update, |db| {
        db.server_settings
            .insert(&create_settings(123, "c1"))
            .await
            .unwrap();

        let updated = create_settings(123, "c2");
        db.server_settings.update(&updated).await.unwrap();

        let fetched = db.server_settings.select(&123).await.unwrap().unwrap();
        assert_eq!(fetched.settings.0.feeds.channel_id, Some("c2".to_string()));
    });

    db_test!(delete, |db| {
        db.server_settings
            .insert(&create_settings(123, "c1"))
            .await
            .unwrap();

        db.server_settings.delete(&123).await.unwrap();
        assert!(db.server_settings.select(&123).await.unwrap().is_none());
    });
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
}
