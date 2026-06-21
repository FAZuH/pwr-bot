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

use deadpool_postgres::ManagerConfig;
use deadpool_postgres::Pool;
use deadpool_postgres::RecyclingMethod;
use pwr_bot_sdk::*;
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

    fn commands(&self) -> Vec<CommandSpec> {
        vec![
            CommandSpec::new("feed", "Manage feed subscriptions"),
            CommandSpec::new("feed list", "List your subscriptions"),
            CommandSpec::new("feed settings", "Configure feed settings"),
            CommandSpec {
                name: "feed subscribe".into(),
                description: "Subscribe to one or more feeds".into(),
                args: vec![
                    ArgSpec {
                        name: "links".into(),
                        description: "Link(s) of the feeds. Separate with commas".into(),
                        kind: "String".into(),
                    },
                    ArgSpec {
                        name: "send_into".into(),
                        description: "Where to send notifications (dm or server)".into(),
                        kind: "String".into(),
                    },
                ],
            },
            CommandSpec {
                name: "feed unsubscribe".into(),
                description: "Unsubscribe from one or more feeds".into(),
                args: vec![
                    ArgSpec {
                        name: "links".into(),
                        description: "Link(s) of the feeds to remove".into(),
                        kind: "String".into(),
                    },
                    ArgSpec {
                        name: "send_into".into(),
                        description: "Where to send notifications (dm or server)".into(),
                        kind: "String".into(),
                    },
                ],
            },
        ]
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
            "component_interaction" => self.handle_component_interaction(payload, host).await,
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

        let total_pages = total.div_ceil(view::PER_PAGE);

        // Create view state
        let state = view::FeedListState::new(subscriber_id, total_pages);

        // Query first page
        let feeds = subscription::list_paginated_subscriptions(
            &pool,
            subscriber_id,
            view::PER_PAGE as i64,
            0,
        )
        .await?;

        // Render and store state
        let response = view::render_feed_list(&state, &feeds);
        self.feed_list_states.lock().await.insert(author_id, state);
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

        let _guild_id = unsafe { host.guild_id() };
        let author_id = unsafe { host.author_id() };
        let _send_into = extract_arg(args, "send_into").unwrap_or("dm");

        let pool = self.pool();

        // Get or create the subscriber
        let subscriber =
            subscription::add_subscriber(&pool, host, _send_into, &author_id.to_string()).await?;
        let subscriber_id = subscriber
            .get("id")
            .and_then(|v| v.as_i64())
            .ok_or("Failed to get subscriber ID")? as i32;

        let platforms = self.platforms.lock().await;

        let mut results: Vec<String> = Vec::new();
        for url in &urls {
            // Resolve platform from URL
            let source_id = match resolve_source_id(platforms.as_deref(), url) {
                Ok(id) => id,
                Err(e) => {
                    results.push(format!("❌ `{url}`: {e}"));
                    continue;
                }
            };

            // Check if feed exists in DB
            let existing = subscription::get_feed_by_source_id(&pool, host, source_id).await?;
            let feed = match existing {
                Some(f) => f,
                None => {
                    results.push(format!("❌ `{url}`: Feed not found. Add it first."));
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

            match subscription::subscribe(&pool, host, feed_id, subscriber_id).await? {
                subscription::SubscribeResult::Success { .. } => {
                    results.push(format!("✅ Subscribed to **{feed_name}**"));
                }
                subscription::SubscribeResult::AlreadySubscribed { .. } => {
                    results.push(format!("ℹ️ Already subscribed to **{feed_name}**"));
                }
            }
        }

        if results.is_empty() {
            return Ok(ResponsePayload::text_ephemeral("No feeds were processed."));
        }

        let content = format!("## Subscribe Results\n{}", results.join("\n"));
        Ok(ResponsePayload::text(content))
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

        let author_id = unsafe { host.author_id() };
        let _send_into = extract_arg(args, "send_into").unwrap_or("dm");

        let pool = self.pool();

        // Find the subscriber
        let subscriber =
            subscription::add_subscriber(&pool, host, _send_into, &author_id.to_string()).await?;
        let subscriber_id = subscriber
            .get("id")
            .and_then(|v| v.as_i64())
            .ok_or("Failed to get subscriber ID")? as i32;

        let platforms_lock = self.platforms.lock().await;
        let platforms = platforms_lock.as_deref();
        let mut results: Vec<String> = Vec::new();
        for url in &urls {
            let source_id = match resolve_source_id(platforms, url) {
                Ok(id) => id,
                Err(e) => {
                    results.push(format!("❌ `{url}`: {e}"));
                    continue;
                }
            };

            let existing = subscription::get_feed_by_source_id(&pool, host, source_id).await?;
            let feed = match existing {
                Some(f) => f,
                None => {
                    results.push(format!("❌ `{url}`: Feed not found."));
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

            match subscription::unsubscribe(&pool, host, feed_id, subscriber_id).await? {
                subscription::UnsubscribeResult::Success { .. } => {
                    results.push(format!("✅ Unsubscribed from **{feed_name}**"));
                }
                subscription::UnsubscribeResult::AlreadyUnsubscribed { .. } => {
                    results.push(format!("ℹ️ Was not subscribed to **{feed_name}**"));
                }
                subscription::UnsubscribeResult::NoneSubscribed { .. } => {
                    results.push(format!("ℹ️ Not subscribed to **{feed_name}**"));
                }
            }
        }

        if results.is_empty() {
            return Ok(ResponsePayload::text_ephemeral("No feeds were processed."));
        }

        let content = format!("## Unsubscribe Results\n{}", results.join("\n"));
        Ok(ResponsePayload::text(content))
    }

    fn pool(&self) -> deadpool_postgres::Pool {
        self.pool
            .try_lock()
            .ok()
            .and_then(|g| g.clone())
            .expect("pool not initialized")
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

        // Select menu interaction — just store the selected values, no re-render
        if action == "select" {
            let values: Vec<String> = payload
                .get("values")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            state.selected_unsub = values;
            return Ok(());
        }

        match action {
            "edit" => {
                state.edit_mode = true;
                state.selected_unsub.clear();
            }
            "cancel" => {
                state.edit_mode = false;
                state.selected_unsub.clear();
            }
            "save" => {
                let selected = std::mem::take(&mut state.selected_unsub);
                state.edit_mode = false;
                let sub_id = state.subscriber_id;
                for feed_id_str in &selected {
                    if let Ok(feed_id) = feed_id_str.parse::<i32>() {
                        let _ = subscription::unsubscribe(&pool, host, feed_id, sub_id).await;
                    }
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
            subscription::list_paginated_subscriptions(&pool, state_clone.subscriber_id, 25, 0)
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

/// Resolves a source_id from a platform URL.
fn resolve_source_id<'a>(
    platforms: Option<&'a crate::platform::Platforms>,
    url: &'a str,
) -> Result<&'a str, String> {
    let platforms = platforms.ok_or("Platforms not initialized")?;
    let platform = platforms
        .get_platform_by_source_url(url)
        .ok_or_else(|| format!("Unsupported URL: {url}"))?;
    platform
        .get_id_from_source_url(url)
        .map_err(|e| format!("Failed to parse URL: {e}"))
}
