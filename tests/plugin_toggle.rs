//! The per-guild plugin toggle as the serving path sees it: a core plugin
//! switched off in one guild is refused there and still served in another.
//!
//! What is exercised is [`Data::plugin_enabled_in`] — the enabled-set
//! resolution every serving gate asks before invoking a plugin (the command
//! gate in `open_plugin_view`, the click gate in the event handler, and
//! [`Data::take_disabled_modal`] for modal submissions) — over the real
//! `guild_plugins` rows. Pinned here because the toggle is only real if the
//! side that serves reads it: a row that resolves correctly while nothing
//! asks still leaves the plugin answering the guild it was disabled for.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use poise::serenity_prelude::GuildId;
use pwr_bot::bot::Data;
use pwr_bot::bot::error::BotError;
use pwr_bot::bot::translate::SettingsReturns;
use pwr_bot::bot::translate::TranslateLayer;
use pwr_bot::config::Config;
use pwr_bot::config::CorePluginSpec;
use pwr_bot::plugin::InteractionEngine;
use pwr_bot::plugin::ModalRouteError;
use pwr_bot::plugin::PluginManager;
use pwr_bot::plugin::RespawnPolicy;
use pwr_bot::plugin::RunningPlugin;
use pwr_bot::repo::PgRepos;
use pwr_bot::repo::traits::Repos;
use pwr_bot::service::Services;

mod common;

/// A host whose auto-enabled plugins are the two core names, over `db`.
/// `core_plugins` is the production source of the auto-enable list
/// (`CORE_PLUGINS`), so a plugin with no row is enabled exactly as it is in a
/// running bot. Nothing is spawned: the gates read rows, not processes.
async fn host(db: Arc<PgRepos>) -> Arc<Data> {
    let repos: Arc<dyn Repos + Send + Sync> = db;
    let config = Config {
        core_plugins: ["feed", "voice"]
            .into_iter()
            .map(|name| CorePluginSpec {
                name: name.to_string(),
                path: PathBuf::from("/srv/pwr-bot/plugins").join(name),
            })
            .collect(),
        ..Default::default()
    };
    let service = Arc::new(
        Services::new(repos.clone())
            .await
            .expect("construct services"),
    );
    Arc::new(Data {
        config: Arc::new(config),
        service,
        repos,
        plugin_manager: Arc::new(PluginManager::new(None, RespawnPolicy::default())),
        plugin_catalog: Arc::new(HashMap::new()),
        plugin_catalog_error: None,
        plugin_engine: Arc::new(InteractionEngine::<RunningPlugin>::new()),
        plugin_routes: Arc::new(HashMap::new()),
        core_manifests: Arc::new(HashMap::new()),
        translate_layer: Arc::new(TranslateLayer::new()),
        settings_returns: Arc::new(SettingsReturns::default()),
        start_time: Instant::now(),
    })
}

/// The gate refuses a core plugin in the guild that switched it off and keeps
/// serving it everywhere else — the whole point of a per-guild toggle, since
/// the plugin is one process shared by every guild.
#[tokio::test]
#[serial_test::serial]
async fn a_core_plugin_disabled_in_one_guild_is_refused_there_and_served_in_the_other() {
    let db = common::setup_db().await;
    let data = host(db.clone()).await;
    let (disabled_guild, serving_guild) = (GuildId::new(7), GuildId::new(8));

    db.guild_plugins()
        .set_enabled(disabled_guild.get(), "feed", false)
        .await
        .expect("`/plugin disable feed` in guild 7");

    assert!(
        !data
            .plugin_enabled_in(Some(disabled_guild), "feed")
            .await
            .expect("read the enabled set"),
        "the guild that switched `feed` off must not be served it"
    );
    assert!(
        data.plugin_enabled_in(Some(serving_guild), "feed")
            .await
            .expect("read the enabled set"),
        "guild 8 has no row, so the shared process keeps serving `feed` there"
    );

    common::teardown_db(&db).await;
}

/// An untouched core plugin is served: the gate reads an absent row as
/// enabled, so it must not refuse every plugin that has never been toggled.
#[tokio::test]
#[serial_test::serial]
async fn a_core_plugin_that_was_never_toggled_is_served() {
    let db = common::setup_db().await;
    let data = host(db.clone()).await;
    let guild = GuildId::new(9);

    assert!(
        data.plugin_enabled_in(Some(guild), "voice")
            .await
            .expect("read the enabled set"),
        "no row means auto-enabled"
    );

    // A DM carries no guild, so there is no per-guild state to gate: a plugin
    // command invoked in one is served rather than refused against nothing.
    assert!(
        data.plugin_enabled_in(None, "voice")
            .await
            .expect("DM read"),
        "a DM has no guild to be disabled in"
    );

    common::teardown_db(&db).await;
}

/// The refusal names the plugin and arrives as a [`BotError`], so the shared
/// error seam renders it as the sentence a user reads. Fails if the refusal
/// is bare text (which would render as an internal error with a reference id)
/// or if it leaves the plugin unnamed, leaving the admin nothing to act on.
#[test]
fn the_disabled_plugin_refusal_names_the_plugin_through_the_shared_error_seam() {
    let error: Box<dyn std::error::Error + Send + Sync> = BotError::PluginDisabledInGuild {
        plugin: "feed".to_string(),
    }
    .into();

    let bot_error = error
        .downcast_ref::<BotError>()
        .expect("the refusal answers through the shared error seam");
    let message = bot_error.to_string();

    assert!(
        message.contains("`feed`"),
        "the refusal names the plugin: {message}"
    );
    assert!(
        !message.contains("not running") && !message.contains("unknown"),
        "the plugin is disabled, not gone: {message}"
    );
}

// ── modal submissions ─────────────────────────────────────────────────────

/// A modal the author opened *before* the toggle must not submit its write
/// after it. Pinned at [`Data::take_disabled_modal`] — the seam the
/// modal-submit handler asks before delivering — because the handler itself
/// needs a real `ModalInteraction` and a live Discord HTTP client, neither of
/// which the harness can build; what the handler adds on top is only the
/// refusal message. The gate's two real behaviours live here: it refuses, and
/// it consumes the route so the same modal cannot be submitted again.
#[tokio::test]
#[serial_test::serial]
async fn a_modal_of_a_plugin_disabled_in_the_guild_is_refused_and_its_route_consumed() {
    let db = common::setup_db().await;
    let data = host(db.clone()).await;
    let guild = GuildId::new(11);

    data.plugin_manager
        .bind_modal(7, "feed", "feed:welcome")
        .await;
    db.guild_plugins()
        .set_enabled(guild.get(), "feed", false)
        .await
        .expect("`/plugins disable feed` in guild 11");

    assert_eq!(
        data.take_disabled_modal(7, Some(guild)).await,
        Some("feed".to_string()),
        "the submission of a disabled plugin is refused, naming the plugin"
    );

    // Consumed: a retried submit finds no route and is delivered nowhere.
    assert_eq!(
        data.plugin_manager.take_modal(7).await.unwrap_err(),
        ModalRouteError::NoBinding(7),
        "the refused submission's route is gone, so it cannot be retried"
    );
    assert_eq!(
        data.take_disabled_modal(7, Some(guild)).await,
        None,
        "a second submit of the same modal has no owner to refuse"
    );

    common::teardown_db(&db).await;
}

/// The same pending modal still submits in a guild that left the plugin on —
/// the shared process serves every other guild, including this one's users.
#[tokio::test]
#[serial_test::serial]
async fn the_same_pending_modal_submits_in_a_guild_that_kept_the_plugin_enabled() {
    let db = common::setup_db().await;
    let data = host(db.clone()).await;
    let disabled_guild = GuildId::new(12);
    let serving_guild = GuildId::new(13);

    data.plugin_manager
        .bind_modal(7, "feed", "feed:welcome")
        .await;
    db.guild_plugins()
        .set_enabled(disabled_guild.get(), "feed", false)
        .await
        .expect("`/plugins disable feed` in guild 12");

    assert_eq!(
        data.take_disabled_modal(7, Some(serving_guild)).await,
        None,
        "guild 13 never switched `feed` off"
    );

    // A DM has no per-guild state either.
    assert_eq!(data.take_disabled_modal(7, None).await, None);

    // Untouched: the gate peeked, so the delivery still finds its route.
    assert_eq!(
        data.plugin_manager
            .take_modal(7)
            .await
            .expect("the served submission keeps its route")
            .owner,
        "feed"
    );

    common::teardown_db(&db).await;
}

/// A submission with no pending modal route is not a plugin submission at
/// all: the gate must leave it alone so the message-keyed route still answers
/// it, exactly as before the disable existed.
#[tokio::test]
#[serial_test::serial]
async fn a_submission_without_a_plugin_route_is_left_to_the_other_routes() {
    let db = common::setup_db().await;
    let data = host(db.clone()).await;
    let guild = GuildId::new(14);

    db.guild_plugins()
        .set_enabled(guild.get(), "feed", false)
        .await
        .expect("`/plugins disable feed` in guild 14");

    assert_eq!(
        data.take_disabled_modal(7, Some(guild)).await,
        None,
        "no route, nothing to refuse: the fall-through is unchanged"
    );

    common::teardown_db(&db).await;
}
