pub mod error;
pub mod platform;
mod publisher;
mod settings_view;
mod subscriber;
pub mod subscription;
pub mod update;
mod view;

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use deadpool_postgres::ManagerConfig;
use pwr_bot_plugin_util::db_execute;
use deadpool_postgres::Pool;
use deadpool_postgres::RecyclingMethod;
use pwr_bot_sdk::*;
use serenity::all::*;
use serenity::builder::CreateCommand;
use serenity::builder::CreateCommandOption;
use serenity::model::application::CommandOptionType;
use tokio::sync::Mutex;

use crate::platform::Platforms;
use crate::update::feed_settings::FeedSettingsModel;
use crate::update::feed_settings::FeedSettingsMsg;
use crate::update::feed_settings::feed_settings_update;

pwr_bot_sdk::export_plugin!(FeedPlugin, FeedPlugin::new());

pub struct FeedPlugin {
    platforms: Mutex<Option<Arc<Platforms>>>,
    pool: Mutex<Option<Pool>>,
    feed_list_states: Mutex<HashMap<u64, view::FeedListState>>,
    feed_settings_states: Mutex<HashMap<u64, settings_view::FeedSettingsState>>,
}

impl FeedPlugin {
    pub fn new() -> Self {
        Self {
            platforms: Mutex::new(None),
            pool: Mutex::new(None),
            feed_list_states: Mutex::new(HashMap::new()),
            feed_settings_states: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for FeedPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BotPlugin for FeedPlugin {
    fn name(&self) -> &'static str {
        "feed"
    }

    fn description(&self) -> &'static str {
        "Feed subscription management"
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        let cmd = CreateCommand::new("feed")
            .description("Feed subscription management")
            .add_option(CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "list",
                "List your subscriptions",
            ))
            .add_option(CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "settings",
                "Configure feed settings",
            ))
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "subscribe",
                    "Subscribe to one or more feeds",
                )
                .add_sub_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "links",
                        "Link(s) of the feeds. Separate with commas",
                    )
                    .required(true),
                )
                .add_sub_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "send_into",
                        "Where to send notifications. Default to DM",
                    )
                    .required(false)
                    .add_string_choice("DM", "dm")
                    .add_string_choice("Server", "server"),
                ),
            )
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "unsubscribe",
                    "Unsubscribe from one or more feeds",
                )
                .add_sub_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "links",
                        "Link(s) of the feeds to remove",
                    )
                    .required(true),
                )
                .add_sub_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "send_into",
                        "Where to send notifications. Default to DM",
                    )
                    .required(false)
                    .add_string_choice("DM", "dm")
                    .add_string_choice("Server", "server"),
                ),
            );

        vec![CommandDefinition {
            name: "feed".into(),
            data: serde_json::to_value(&cmd).expect("CreateCommand serialization"),
        }]
    }

    fn settings_panels(&self) -> Vec<SettingsPanelSpec> {
        vec![SettingsPanelSpec::new("feeds", "Feeds")]
    }

    fn tasks(&self) -> Vec<TaskSpec> {
        vec![TaskSpec::new("feed-publisher", 60, "__poll_feeds")]
    }

    fn test_steps(&self) -> Vec<TestStepSpec> {
        vec![
            TestStepSpec::new(
                "feed list",
                "Subscription list (empty)",
                "feed list",
                serde_json::json!({"action": "list"}),
            ),
            TestStepSpec::new(
                "feed settings",
                "Feed settings panel render",
                "feed settings",
                serde_json::json!({"action": "settings"}),
            ),
        ]
    }

    fn event_handlers(&self) -> Vec<EventHandlerSpec> {
        vec![
            EventHandlerSpec::new("feed_update".to_string()),
            EventHandlerSpec::new("component_interaction".to_string()),
            EventHandlerSpec::new("view_timeout".to_string()),
        ]
    }

    async fn init(&self, _host: &PluginHost) -> Result<(), String> {
        let db_url = std::env::var("DB_URL")
            .map_err(|_| "DB_URL environment variable not set".to_string())?;
        let mut config = deadpool_postgres::Config::new();
        config.url = Some(db_url);
        config.manager = Some(ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        });
        let pool = config
            .create_pool(
                Some(deadpool_postgres::Runtime::Tokio1),
                tokio_postgres::NoTls,
            )
            .map_err(|e| format!("Failed to create database pool: {e}"))?;
        *self.pool.lock().await = Some(pool);

        let platforms = Arc::new(Platforms::new());
        *self.platforms.lock().await = Some(platforms);
        Ok(())
    }

    async fn on_event(
        &self,
        event_name: &str,
        payload: serde_json::Value,
        host: &PluginHost,
    ) -> Result<(), String> {
        match event_name {
            "feed_update" => {
                let pool = self
                    .pool
                    .lock()
                    .await
                    .as_ref()
                    .ok_or("Database pool not initialized")?
                    .clone();
                subscriber::handle_feed_update(&pool, host, payload).await
            }
            "component_interaction" => self
                .handle_component_interaction(payload, host)
                .await
                .map_err(|e| {
                    tracing::error!("component_interaction handler failed: {e}");
                    eprintln!("component_interaction handler failed: {e}");
                    e
                }),
            "view_timeout" => {
                let msg_id = payload
                    .get("message_id")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if msg_id > 0 {
                    self.feed_list_states.lock().await.remove(&msg_id);
                }
                // Also clean settings state by guild_id from the payload
                if let Some(gid) = payload.get("guild_id").and_then(|v| v.as_u64()) {
                    self.feed_settings_states.lock().await.remove(&gid);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn invoke(
        &self,
        command: &str,
        args: serde_json::Value,
        host: &PluginHost,
    ) -> Result<ResponsePayload, String> {
        match command {
            "feed" | "feed list" => self.cmd_list(host).await,
            "feed settings" => self.cmd_settings(host).await,
            "feed subscribe" => self.cmd_subscribe(host, &args).await,
            "feed unsubscribe" => self.cmd_unsubscribe(host, &args).await,
            "__poll_feeds" => self.cmd_poll_feeds(host).await,
            _ => Err(format!("Unknown command: {command}")),
        }
    }
}

impl FeedPlugin {
    async fn cmd_settings(&self, host: &PluginHost) -> Result<ResponsePayload, String> {
        unsafe { host.defer().map_err(|e| e.to_string())? };

        let guild_id = unsafe { host.guild_id() };
        if guild_id == 0 {
            return Ok(ResponsePayload::text_ephemeral(
                "This command can only be used in a server.",
            ));
        }

        let settings = subscription::get_server_settings(&self.pool(), guild_id).await?;

        let feeds = settings.get("feeds");

        let model = FeedSettingsModel {
            enabled: feeds.and_then(|f| f.get("enabled").and_then(|v| v.as_bool())),
            channel_id: feeds
                .and_then(|f| f.get("channel_id"))
                .and_then(|v| v.as_str())
                .map(String::from),
            subscribe_role_id: feeds
                .and_then(|f| f.get("subscribe_role_id"))
                .and_then(|v| v.as_str())
                .map(String::from),
            unsubscribe_role_id: feeds
                .and_then(|f| f.get("unsubscribe_role_id"))
                .and_then(|v| v.as_str())
                .map(String::from),
        };

        let state = settings_view::FeedSettingsState::new(model, guild_id);
        let response = settings_view::render_feed_settings(&state);
        self.feed_settings_states
            .lock()
            .await
            .insert(guild_id, state);
        Ok(response)
    }

    async fn cmd_list(&self, host: &PluginHost) -> Result<ResponsePayload, String> {
        unsafe { host.defer().map_err(|e| e.to_string())? };

        let author_id = unsafe { host.author_id() };
        let target_id = author_id.to_string();

        let pool = self.pool();

        // Find the subscriber
        let result = subscription::get_subscriber_by_target(&pool, &target_id).await?;

        let subscriber_id = match result {
            Some(id) => id,
            None => {
                return Ok(ResponsePayload::text(
                    "You have no subscriptions. Use `/feed subscribe` to add some!",
                ));
            }
        };

        // Count total subscriptions
        let total = subscription::count_subscriptions(&pool, subscriber_id).await?;
        if total == 0 {
            return Ok(ResponsePayload::text(
                "You have no subscriptions. Use `/feed subscribe` to add some!",
            ));
        }

        // Query first page
        let mut feeds = subscription::list_paginated_subscriptions(
            &pool,
            subscriber_id,
            view::PER_PAGE as i64,
            0,
        )
        .await?;

        self.backfill_feeds(&pool, &mut feeds, "cmd_list").await;

        // Re-count total subscriptions (might have changed after backfill)
        let total = subscription::count_subscriptions(&pool, subscriber_id).await?;
        let total_pages = total.div_ceil(view::PER_PAGE).max(1);

        // Render and store state
        let state = view::FeedListState::new(subscriber_id, total_pages);
        let response = view::render_feed_list(&state, &feeds);
        self.feed_list_states
            .lock()
            .await
            .insert(author_id, state);
        Ok(response)
    }

    async fn cmd_poll_feeds(&self, host: &PluginHost) -> Result<ResponsePayload, String> {
        let pool = self
            .pool
            .lock()
            .await
            .as_ref()
            .ok_or("Database pool not initialized")?
            .clone();
        let platforms = self.platforms.lock().await;
        let platforms = platforms.as_ref().ok_or("Platforms not initialized")?;
        publisher::poll_feeds(&pool, host, platforms).await?;
        Ok(ResponsePayload::text(""))
    }

    async fn cmd_subscribe(
        &self,
        host: &PluginHost,
        args: &serde_json::Value,
    ) -> Result<ResponsePayload, String> {
        unsafe { host.defer().map_err(|e| e.to_string())? };

        let links = extract_arg(args, "links").unwrap_or_default();
        let urls: Vec<&str> = links
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if urls.is_empty() {
            return Ok(ResponsePayload::text_ephemeral(
                "Please provide at least one feed URL.",
            ));
        }

        let guild_id = unsafe { host.guild_id() };
        let author_id = unsafe { host.author_id() };
        let send_into = extract_arg(args, "send_into").unwrap_or("dm");

        let (sub_type, target_id) = match resolve_subscriber_target(send_into, guild_id, author_id)
        {
            Ok(v) => v,
            Err(msg) => return Ok(ResponsePayload::text_ephemeral(msg)),
        };

        let pool = self.pool();

        // Get or create the subscriber
        let subscriber = subscription::add_subscriber(&pool, host, sub_type, &target_id).await?;
        let subscriber_id = subscriber
            .get("id")
            .and_then(|v| v.as_i64())
            .ok_or("Failed to get subscriber ID")? as i32;

        let platforms = self.platforms.lock().await;

        // Build initial progress states
        let mut states: Vec<String> = vec!["⏳ Processing...".to_string(); urls.len()];

        for (i, url) in urls.iter().enumerate() {
            // Resolve platform and source ID from URL
            let (platform, source_id) =
                match resolve_platform_and_source_id(platforms.as_deref(), url) {
                    Ok(v) => v,
                    Err(e) => {
                        states[i] = format!("❌ `{url}`: {e}");
                        continue;
                    }
                };

            // Get or create the feed (fetches from platform API if needed)
            let feed = match subscription::get_or_create_feed(
                &pool,
                host,
                platform.as_ref(),
                url,
                source_id,
            )
            .await
            {
                Ok(f) => f,
                Err(e) => {
                    states[i] = format!("❌ `{url}`: {e}");
                    continue;
                }
            };

            let feed_id = feed
                .get("id")
                .and_then(|v| v.as_i64())
                .ok_or("Invalid feed data")? as i32;
            let feed_name = feed
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();
            let feed_url = feed
                .get("source_url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            match subscription::subscribe(&pool, host, feed_id, subscriber_id).await? {
                subscription::SubscribeResult::Success { .. } => {
                    states[i] =
                        format!("✅ **Successfully** subscribed to [{feed_name}](<{feed_url}>)");
                }
                subscription::SubscribeResult::AlreadySubscribed { .. } => {
                    states[i] =
                        format!("❌ You are **already subscribed** to [{feed_name}](<{feed_url}>)");
                }
            }

            let is_final = i + 1 == urls.len();
            if is_final {
                return Ok(render_batch_payload("Subscribe Results", &states, true));
            }
        }

        Ok(ResponsePayload::text_ephemeral("No feeds were processed."))
    }

    async fn cmd_unsubscribe(
        &self,
        host: &PluginHost,
        args: &serde_json::Value,
    ) -> Result<ResponsePayload, String> {
        unsafe { host.defer().map_err(|e| e.to_string())? };

        let links = extract_arg(args, "links").unwrap_or_default();
        let urls: Vec<&str> = links
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if urls.is_empty() {
            return Ok(ResponsePayload::text_ephemeral(
                "Please provide at least one feed URL.",
            ));
        }

        let guild_id = unsafe { host.guild_id() };
        let author_id = unsafe { host.author_id() };
        let send_into = extract_arg(args, "send_into").unwrap_or("dm");

        let (sub_type, target_id) = match resolve_subscriber_target(send_into, guild_id, author_id)
        {
            Ok(v) => v,
            Err(msg) => return Ok(ResponsePayload::text_ephemeral(msg)),
        };

        let pool = self.pool();

        // Find the subscriber
        let subscriber = subscription::add_subscriber(&pool, host, sub_type, &target_id).await?;
        let subscriber_id = subscriber
            .get("id")
            .and_then(|v| v.as_i64())
            .ok_or("Failed to get subscriber ID")? as i32;

        let platforms_lock = self.platforms.lock().await;

        // Build initial progress states
        let mut states: Vec<String> = vec!["⏳ Processing...".to_string(); urls.len()];

        for (i, url) in urls.iter().enumerate() {
            let (platform, source_id) =
                match resolve_platform_and_source_id(platforms_lock.as_deref(), url) {
                    Ok(v) => v,
                    Err(e) => {
                        states[i] = format!("❌ `{url}`: {e}");
                        continue;
                    }
                };

            let feed = match subscription::get_or_create_feed(
                &pool,
                host,
                platform.as_ref(),
                url,
                source_id,
            )
            .await
            {
                Ok(f) => f,
                Err(e) => {
                    states[i] = format!("❌ `{url}`: {e}");
                    continue;
                }
            };

            let feed_id = feed
                .get("id")
                .and_then(|v| v.as_i64())
                .ok_or("Invalid feed data")? as i32;
            let feed_name = feed
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();
            let feed_url = feed
                .get("source_url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            match subscription::unsubscribe(&pool, host, feed_id, subscriber_id).await? {
                subscription::UnsubscribeResult::Success { .. } => {
                    states[i] = format!(
                        "✅ **Successfully** unsubscribed from [{feed_name}](<{feed_url}>)"
                    );
                }
                subscription::UnsubscribeResult::AlreadyUnsubscribed { .. }
                | subscription::UnsubscribeResult::NoneSubscribed { .. } => {
                    states[i] =
                        format!("❌ You are **not subscribed** to [{feed_name}](<{feed_url}>)");
                }
            }

            let is_final = i + 1 == urls.len();
            if is_final {
                return Ok(render_batch_payload("Unsubscribe Results", &states, true));
            }
        }

        Ok(ResponsePayload::text_ephemeral("No feeds were processed."))
    }

    fn pool(&self) -> deadpool_postgres::Pool {
        self.pool
            .try_lock()
            .ok()
            .and_then(|g| g.clone())
            .expect("pool not initialized")
    }

    /// Lazy backfill: for feeds with no `latest_title` (no `feed_items` row),
    /// try to fetch the latest item from the platform API. On failure, insert
    /// a placeholder so the feed never perpetually shows "No latest version found."
    async fn backfill_feeds(
        &self,
        pool: &deadpool_postgres::Pool,
        feeds: &mut [serde_json::Value],
        context: &str,
    ) {
        let platforms_lock = self.platforms.lock().await;
        let Some(ref platforms) = *platforms_lock else {
            return;
        };
        let all_platforms = platforms.get_all_platforms();
        for feed in feeds.iter_mut() {
            let has_title = feed
                .get("latest_title")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty());
            if has_title {
                continue;
            }
            let feed_id = feed.get("id").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let platform_name = feed.get("platform_id").and_then(|v| v.as_str()).unwrap_or("");
            let items_id = feed.get("items_id").and_then(|v| v.as_str()).unwrap_or("");
            if feed_id == 0 || items_id.is_empty() {
                continue;
            }
            if let Some(platform) = all_platforms.iter().find(|p| p.get_id() == platform_name) {
                match platform.fetch_latest(items_id).await {
                    Ok(latest) => {
                        let _ = db_execute(
                            pool,
                            "INSERT INTO feed_items (feed_id, description, published) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
                            &[&feed_id, &latest.title, &latest.published.to_rfc3339()],
                        )
                        .await;
                        feed["latest_title"] = serde_json::json!(latest.title);
                        feed["latest_published"] = serde_json::json!(latest.published.timestamp());
                    }
                    Err(e) => {
                        eprintln!("{context}: fetch_latest failed for feed_id={feed_id} items_id={items_id}: {e}");
                        let _ = db_execute(
                            pool,
                            "INSERT INTO feed_items (feed_id, description, published) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
                            &[&feed_id, &"", &Utc::now().to_rfc3339()],
                        )
                        .await;
                    }
                }
            }
        }
        drop(platforms_lock);
    }

    /// Handles a component interaction for the feed list view.
    ///
    /// Parses the `custom_id` to determine the action (pagination, edit, save,
    /// cancel, or select), updates the view state, re-queries the DB if needed,
    /// and edits the message via `host.edit_reply()`.
    async fn handle_component_interaction(
        &self,
        payload: serde_json::Value,
        host: &PluginHost,
    ) -> Result<(), String> {
        let custom_id = payload
            .get("custom_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let message_id = payload
            .get("message_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let user_id = payload.get("user_id").and_then(|v| v.as_u64()).unwrap_or(0);

        if custom_id.starts_with(settings_view::SETTINGS_CUSTOM_ID_PREFIX) {
            return self.handle_settings_interaction(payload, host).await;
        }
        if !custom_id.starts_with(view::CUSTOM_ID_PREFIX) {
            return Ok(());
        }

        let action = &custom_id[view::CUSTOM_ID_PREFIX.len()..];
        let pool = self.pool();

        // Handle view_subs early — it comes from the batch message which is
        // not in feed_list_states.
        if action == "view_subs" {
            let author_id = unsafe { host.author_id() };
            let target_id = author_id.to_string();
            let result = subscription::get_subscriber_by_target(&pool, &target_id).await?;
            let subscriber_id = match result {
                Some(id) => id,
                None => return Ok(()),
            };
            let total = subscription::count_subscriptions(&pool, subscriber_id)
                .await
                .unwrap_or(0);
            let total_pages = total.div_ceil(view::PER_PAGE).max(1);
            let mut feeds = subscription::list_paginated_subscriptions(
                &pool,
                subscriber_id,
                view::PER_PAGE as i64,
                0,
            )
            .await
            .unwrap_or_default();
            self.backfill_feeds(&pool, &mut feeds, "view_subs").await;
            let list_state = view::FeedListState::new(subscriber_id, total_pages);
            let response = view::render_feed_list(&list_state, &feeds);
            let json_str = serde_json::to_string(&response).map_err(|e| e.to_string())?;
            unsafe {
                host.edit_reply(message_id, &json_str)
                    .map_err(|e| e.to_string())?
            };
            return Ok(());
        }

        // On first interaction, re-key state from author_id to message_id
        // so that view_timeout can find and remove it.
        let mut states = self.feed_list_states.lock().await;
        let state = match states.get_mut(&message_id) {
            Some(s) => s,
            None => {
                let moved = states.remove(&user_id).map(|s| {
                    states.insert(message_id, s);
                    states.get_mut(&message_id).unwrap()
                });
                match moved {
                    Some(s) => s,
                    None => return Ok(()),
                }
            }
        };

        match action {
            "edit" => {
                state.edit_mode = true;
                state.selected_unsub.clear();
            }
            "view" => {
                state.edit_mode = false;
                state.selected_unsub.clear();
            }
            s if s.starts_with("unsub:") => {
                if let Ok(feed_id) = s[5..].parse::<i32>() {
                    state.selected_unsub.push(feed_id);
                }
                return Ok(()); // No re-render needed, just toggle
            }
            s if s.starts_with("undo:") => {
                if let Ok(feed_id) = s[5..].parse::<i32>() {
                    state.selected_unsub.retain(|&id| id != feed_id);
                }
                return Ok(()); // No re-render needed, just toggle
            }
            "save" => {
                let selected = std::mem::take(&mut state.selected_unsub);
                state.edit_mode = false;
                let sub_id = state.subscriber_id;
                for &feed_id in &selected {
                    let _ = subscription::unsubscribe(&pool, host, feed_id, sub_id).await;
                }
                let total = subscription::count_subscriptions(&pool, sub_id).await?;
                state.total_pages = total.div_ceil(view::PER_PAGE);
                state.page = state.page.min(state.total_pages.max(1));
            }
            s if s.starts_with("pg:") => {
                let page: u32 = s[3..].parse().unwrap_or(1);
                state.page = page.clamp(1, state.total_pages.max(1));
            }
            _ => return Ok(()),
        }

        let state_clone = state.clone();
        drop(states);

        // Query feeds based on mode
        let feeds = if state_clone.edit_mode {
            subscription::list_paginated_subscriptions(
                &pool,
                state_clone.subscriber_id,
                view::EDIT_PER_PAGE as i64,
                0,
            )
            .await?
        } else {
            let offset = ((state_clone.page - 1) * view::PER_PAGE) as i64;
            subscription::list_paginated_subscriptions(
                &pool,
                state_clone.subscriber_id,
                view::PER_PAGE as i64,
                offset,
            )
            .await?
        };

        // Render and edit the message
        let response = view::render_feed_list(&state_clone, &feeds);
        let json_str = serde_json::to_string(&response).map_err(|e| e.to_string())?;
        unsafe {
            host.edit_reply(message_id, &json_str)
                .map_err(|e| e.to_string())?
        };

        Ok(())
    }

    async fn handle_settings_interaction(
        &self,
        payload: serde_json::Value,
        host: &PluginHost,
    ) -> Result<(), String> {
        let custom_id = payload
            .get("custom_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let message_id = payload
            .get("message_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let action = &custom_id[settings_view::SETTINGS_CUSTOM_ID_PREFIX.len()..];

        let guild_id = payload
            .get("guild_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let mut states = self.feed_settings_states.lock().await;

        // Handle view_timeout re-keying: on first interaction, find state by guild_id.
        // Also try guild_id from payload.
        let state_key = if states.contains_key(&guild_id) {
            guild_id
        } else if let Some(gid) = payload.get("guild_id").and_then(|v| v.as_u64()) {
            gid
        } else {
            return Ok(());
        };

        match action {
            "toggle" => {
                let state = match states.get_mut(&state_key) {
                    Some(s) => s,
                    None => return Ok(()),
                };
                feed_settings_update(FeedSettingsMsg::ToggleEnabled, &mut state.model);
                let response = settings_view::render_feed_settings(state);
                let json_str = serde_json::to_string(&response).map_err(|e| e.to_string())?;
                drop(states);
                unsafe {
                    host.edit_reply(message_id, &json_str)
                        .map_err(|e| e.to_string())?
                };
            }
            "channel" => {
                let state = match states.get_mut(&state_key) {
                    Some(s) => s,
                    None => return Ok(()),
                };
                let channel_id = payload
                    .get("values")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.as_str())
                    .map(String::from);
                feed_settings_update(FeedSettingsMsg::SetChannel(channel_id), &mut state.model);
                let response = settings_view::render_feed_settings(state);
                let json_str = serde_json::to_string(&response).map_err(|e| e.to_string())?;
                drop(states);
                unsafe {
                    host.edit_reply(message_id, &json_str)
                        .map_err(|e| e.to_string())?
                };
            }
            "sub_role" => {
                let state = match states.get_mut(&state_key) {
                    Some(s) => s,
                    None => return Ok(()),
                };
                let role_id = payload
                    .get("values")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.as_str())
                    .map(String::from);
                feed_settings_update(FeedSettingsMsg::SetSubRole(role_id), &mut state.model);
                let response = settings_view::render_feed_settings(state);
                let json_str = serde_json::to_string(&response).map_err(|e| e.to_string())?;
                drop(states);
                unsafe {
                    host.edit_reply(message_id, &json_str)
                        .map_err(|e| e.to_string())?
                };
            }
            "unsub_role" => {
                let state = match states.get_mut(&state_key) {
                    Some(s) => s,
                    None => return Ok(()),
                };
                let role_id = payload
                    .get("values")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.as_str())
                    .map(String::from);
                feed_settings_update(FeedSettingsMsg::SetUnsubRole(role_id), &mut state.model);
                let response = settings_view::render_feed_settings(state);
                let json_str = serde_json::to_string(&response).map_err(|e| e.to_string())?;
                drop(states);
                unsafe {
                    host.edit_reply(message_id, &json_str)
                        .map_err(|e| e.to_string())?
                };
            }
            "save" => {
                let state = states.remove(&state_key);
                let state = match state {
                    Some(s) => s,
                    None => return Ok(()),
                };
                drop(states);

                let feeds = serde_json::json!({
                    "enabled": state.model.enabled,
                    "channel_id": state.model.channel_id,
                    "subscribe_role_id": state.model.subscribe_role_id,
                    "unsubscribe_role_id": state.model.unsubscribe_role_id,
                });

                subscription::save_feed_settings(&self.pool(), state.guild_id, &feeds).await?;

                let response = ResponsePayload::text("✅ Feed settings saved.");
                let json_str = serde_json::to_string(&response).map_err(|e| e.to_string())?;
                unsafe {
                    host.edit_reply(message_id, &json_str)
                        .map_err(|e| e.to_string())?
                };
            }
            "cancel" => {
                states.remove(&state_key);
                drop(states);
                let response = ResponsePayload::text("Settings edit cancelled.");
                let json_str = serde_json::to_string(&response).map_err(|e| e.to_string())?;
                unsafe {
                    host.edit_reply(message_id, &json_str)
                        .map_err(|e| e.to_string())?
                };
            }
            _ => {}
        }

        Ok(())
    }
}

/// Extracts a named argument from the serialized CommandData JSON.
///
/// For subcommands, walks `options[0].options` to find the matching arg.
fn extract_arg<'a>(args: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    let opts = args
        .get("options")?
        .as_array()?
        .first()?
        .get("options")?
        .as_array()?;
    for opt in opts {
        if opt.get("name").and_then(|n| n.as_str()) == Some(name) {
            return opt.get("value").and_then(|v| v.as_str());
        }
    }
    None
}

/// Resolves a platform and source_id from a platform URL.
fn resolve_platform_and_source_id<'a>(
    platforms: Option<&'a crate::platform::Platforms>,
    url: &'a str,
) -> Result<(&'a std::sync::Arc<dyn crate::platform::Platform>, &'a str), String> {
    let platforms = platforms.ok_or("Platforms not initialized")?;
    let platform = platforms
        .get_platform_by_source_url(url)
        .ok_or_else(|| format!("Unsupported URL: {url}"))?;
    let source_id = platform
        .get_id_from_source_url(url)
        .map_err(|e| format!("Failed to parse URL: {e}"))?;
    Ok((platform, source_id))
}

/// Resolves the subscriber type and target ID from the `send_into` argument.
///
/// - `"Server"` (or `"server"`) → subscriber type `"guild"`, target ID is the guild ID
/// - `"DM"` (or `"dm"`, or unset) → subscriber type `"dm"`, target ID is the author ID
///
/// Returns an error message when `"server"` is requested outside a guild.
fn resolve_subscriber_target(
    send_into: &str,
    guild_id: u64,
    author_id: u64,
) -> Result<(&'static str, String), String> {
    match send_into.to_lowercase().as_str() {
        "server" => {
            if guild_id == 0 {
                return Err(
                    "This command can only be used in a server when sending to server.".to_string(),
                );
            }
            Ok(("guild", guild_id.to_string()))
        }
        _ => Ok(("dm", author_id.to_string())),
    }
}

/// Builds a Components V2 batch message matching the main branch UI.
///
/// Each state is rendered as an individual `TextDisplay` inside a `Container`.
/// When `is_final`, a `View Subscriptions` secondary button is appended.
fn render_batch_payload(title: &str, states: &[String], is_final: bool) -> ResponsePayload {
    let text_components: Vec<CreateContainerComponent> = states
        .iter()
        .map(|s| CreateContainerComponent::TextDisplay(CreateTextDisplay::new(s.clone())))
        .collect();

    let mut components = vec![CreateComponent::Container(CreateContainer::new(
        text_components,
    ))];

    if is_final {
        let view_btn = CreateButton::new("feed:view_subs")
            .label("View Subscriptions")
            .style(ButtonStyle::Secondary);
        components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![view_btn].into(),
        )));
    }

    let msg = CreateMessage::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(components);

    ResponsePayload::from_serializable(&msg).unwrap_or_else(|_| ResponsePayload::text(title))
}
