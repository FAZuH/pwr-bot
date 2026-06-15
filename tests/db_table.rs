// Shared test modules (common/) are compiled independently per integration test
// binary — each binary sees different subsets as "unused".
#![allow(dead_code)]

use std::collections::HashMap;

use pwr_bot::entity::DbU64;
use pwr_bot::entity::Json;
use pwr_bot::entity::ServerSettings;
use pwr_bot::entity::ServerSettingsEntity;
use pwr_bot::repo::traits::*;

mod common;

macro_rules! db_test {
    ($name:ident, |$db:ident| $body:block) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() {
            let $db = common::setup_db().await;
            $body
            common::teardown_db(&$db).await;
        }
    };
}

mod server_settings_table_tests {
    use super::*;

    fn make_settings(guild_id: u64) -> ServerSettingsEntity {
        let mut plugin_settings = HashMap::new();
        plugin_settings.insert("feeds".to_string(), serde_json::json!({"enabled": true}));
        plugin_settings.insert("voice".to_string(), serde_json::json!({"enabled": false}));
        plugin_settings.insert("welcome".to_string(), serde_json::json!({"enabled": true}));

        ServerSettingsEntity {
            guild_id: DbU64(guild_id),
            settings: Json(ServerSettings { plugin_settings }),
        }
    }

    db_test!(insert_and_select, |db| {
        let entity = make_settings(100);
        let id = db.server_settings.insert(&entity).await.unwrap();
        assert_eq!(id, 100);

        let fetched = db.server_settings.select(&100).await.unwrap().unwrap();
        assert_eq!(fetched.guild_id.0, 100);
        assert!(fetched.settings.0.is_enabled("feeds"));
        assert!(!fetched.settings.0.is_enabled("voice"));
        assert!(fetched.settings.0.is_enabled("welcome"));
    });

    db_test!(update, |db| {
        let entity = make_settings(200);
        db.server_settings.insert(&entity).await.unwrap();

        let mut updated = make_settings(200);
        updated.settings.0.set_enabled("feeds", false);
        db.server_settings.update(&updated).await.unwrap();

        let fetched = db.server_settings.select(&200).await.unwrap().unwrap();
        assert!(!fetched.settings.0.is_enabled("feeds"));
    });

    db_test!(delete, |db| {
        let entity = make_settings(300);
        db.server_settings.insert(&entity).await.unwrap();

        db.server_settings.delete(&300).await.unwrap();

        let fetched = db.server_settings.select(&300).await.unwrap();
        assert!(fetched.is_none());
    });
}
