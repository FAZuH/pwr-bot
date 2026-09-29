//! End-to-end test for the host-session exit seam (`#165`): the Router's
//! Settings section handoff adopts a live host message into a section's
//! panel plugin view. The plugin is spawned over the real stdio wire and owns
//! its test database; the Discord edit is a mock ([`MockHostIo`]).
//!
//! Assertions mirror the handoff contract:
//! - `adopt_message_into_section` invokes the section's command, morphs the
//!   message in place through the Discord-edit seam (the payload is
//!   Components V2 carrying the feed panel), and only then registers the
//!   engine session on the message id;
//! - the adopted session behaves like any panel: a toggle click re-renders
//!   the panel with the flipped state, no host call in between.

use std::path::PathBuf;
use std::sync::Arc;

use feed::repo::Repository;
use feed::service::feed_settings::FeedSettingsService;
use mockall::predicate::eq;
use poise::serenity_prelude as serenity;
use pwr_bot::bot::command::session_exit::adopt_message_into_section;
use pwr_bot::plugin::HostConfig;
use pwr_bot::plugin::HostServices;
use pwr_bot::plugin::InteractionEngine;
use pwr_bot::plugin::PluginManager;
use pwr_bot::plugin::RespawnPolicy;
use pwr_bot::plugin::StatsHandle;
use pwr_bot::plugin::host::MockHostIo;
use pwr_plugin_protocol::FeedsSettings;
use pwr_poise_components::IS_COMPONENTS_V2;
use serde_json::Value;
use serde_json::json;

mod probe;
use probe::probe_binary;
#[path = "support/db.rs"]
mod support_db;

/// The guild the panel keys its settings by.
const GUILD_ID: u64 = 42;

fn admin_context() -> Value {
    json!({
        "user_id": 7,
        "member_roles": [],
        "member_permissions": 1 << 5,
    })
}

fn services(io: Arc<MockHostIo>, db_url: String) -> Arc<HostServices> {
    Arc::new(HostServices {
        io: Some(io),
        config: Some(HostConfig {
            db_url,
            data_path: PathBuf::from("/tmp/pwr-bot-test"),
            poll_interval: std::time::Duration::from_secs(30),
        }),
        kv: None,
        engine: None,
        stats: Arc::new(StatsHandle::default()),
        users: Default::default(),
        welcome: None,
        previews: None,
        settings_returns: None,
    })
}

/// The section handoff adopts a live message into the feed panel: the
/// invoke loads the guild snapshot through the feed seam, one validated edit
/// morphs the message in place, and the message id becomes a plugin session
/// that answers a toggle like any panel.
#[tokio::test]
async fn the_adopted_message_answers_toggles_like_any_panel() {
    let channel_id = 987_654_321_u64;
    let message_id = 123_456_789_u64;

    let db_url = support_db::db_url().await;
    let repository = Repository::connect(&db_url)
        .await
        .expect("connect feed plugin database");
    repository.migrate().await.expect("migrate feed database");
    FeedSettingsService::new(repository)
        .update(
            GUILD_ID,
            FeedsSettings {
                enabled: Some(true),
                channel_id: Some("123456789".into()),
                subscribe_role_id: Some("987654321".into()),
                unsubscribe_role_id: None,
            },
        )
        .await
        .expect("seed feed settings");

    let mut mock = MockHostIo::new();
    mock.expect_edit_message()
        .with(
            eq(channel_id),
            eq(message_id),
            mockall::predicate::function(|data: &Value| {
                data["flags"] == json!(IS_COMPONENTS_V2)
                    && data["components"][0]["components"][0]["content"]
                        == json!("-# **Settings > Feeds**\n## Feed Subscription Settings\n\n> 🛈  Feed notifications are currently **active**. Notifications will be sent to <#123456789>")
                    // The panel view is a create envelope: the morph strips
                    // its create-only fields, which Discord rejects on edit.
                    && data.get("sticker_ids").is_none()
                    && data.get("tts").is_none()
                    && data.get("enforce_nonce").is_none()
            }),
            mockall::predicate::always(),
        )
        .times(1)
        .returning(move |_, _, _, _| Ok(Some(json!({ "message_id": message_id }))));

    let io = Arc::new(mock);
    let engine = InteractionEngine::new();
    let manager = Arc::new(
        PluginManager::new(None, RespawnPolicy::default())
            .with_host_services(services(io.clone(), db_url)),
    );
    let panel = manager
        .spawn("feed", probe_binary("feed"), None, &[], &[])
        .await
        .expect("spawn feed panel");

    let mut message = serenity::Message::default();
    message.channel_id = serenity::ChannelId::new(channel_id).into();
    message.id = serenity::MessageId::new(message_id);
    adopt_message_into_section(
        &engine,
        panel.clone(),
        "feed-settings",
        json!({ "guild_id": GUILD_ID, "_context": admin_context() }),
        io.as_ref(),
        Some(&pwr_bot::plugin::preview::PreviewResolver::new(Vec::new())),
        &message,
    )
    .await
    .expect("the section handoff adopts the message");

    let message_id = serenity::MessageId::new(message_id);
    assert!(
        engine.has_session(message_id).await,
        "the adopted message carries a plugin session"
    );

    let toggled = engine
        .interact_validated(
            message_id,
            "feeds:toggle",
            json!({ "_context": admin_context() }),
            pwr_bot::plugin::validate_view_spec,
        )
        .await
        .expect("the toggle click routes to the adopted session");
    assert_eq!(
        toggled.data["components"][0]["components"][0]["content"],
        json!(
            "-# **Settings > Feeds**\n## Feed Subscription Settings\n\n> 🛈  Feed notifications are currently **paused**. No notifications will be sent until it is re-enabled."
        ),
        "the toggle re-renders the panel with the flipped state"
    );

    let status = panel.stop().await.expect("graceful stop");
    assert_eq!(status.code(), Some(0), "clean exit after bye: {status}");
}
