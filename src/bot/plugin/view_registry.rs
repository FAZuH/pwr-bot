//! Registry for tracking plugin interactive view messages.
//!
//! When a plugin sends a response with Discord components (buttons, select menus),
//! the host registers the message here, routes subsequent component interactions
//! back to the plugin via [`on_event`](pwr_bot_sdk::BotPlugin::on_event), and
//! unregisters the view after a timeout period.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude::*;
use tokio::sync::RwLock;

use crate::bot::Data;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation;
use crate::bot::plugin::registry::PluginRegistry;

const VIEW_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone)]
struct ViewEntry {
    plugin_name: String,
    author_id: UserId,
}

/// Manages the lifecycle of plugin interactive views.
///
/// Tracks messages that contain interactive components sent by plugins,
/// routes component interactions back to the originating plugin, and
/// removes stale views after a timeout.
pub struct PluginViewRegistry {
    views: Arc<RwLock<HashMap<MessageId, ViewEntry>>>,
    http: Arc<Http>,
    plugin_registry: Arc<PluginRegistry>,
}

impl PluginViewRegistry {
    /// Creates a new view registry.
    pub fn new(http: Arc<Http>, plugin_registry: Arc<PluginRegistry>) -> Self {
        Self {
            views: Arc::new(RwLock::new(HashMap::new())),
            http,
            plugin_registry,
        }
    }

    /// Registers a plugin view message and starts the timeout countdown.
    ///
    /// The author_id is used to filter component interactions — only the
    /// original command author may interact with the view.
    pub async fn register(&self, plugin_name: &str, msg_id: MessageId, author_id: UserId) {
        tracing::debug!(
            message.id = %msg_id,
            plugin.name = %plugin_name,
            "registering plugin view",
        );

        self.views.write().await.insert(
            msg_id,
            ViewEntry {
                plugin_name: plugin_name.to_string(),
                author_id,
            },
        );

        // Spawn timeout task
        let views = self.views.clone();
        let plugin_registry = self.plugin_registry.clone();
        let plugin_name_owned = plugin_name.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(VIEW_TIMEOUT).await;
            let entry = views.write().await.remove(&msg_id);
            if let Some(entry) = entry {
                tracing::debug!(
                    message.id = %msg_id,
                    plugin.name = %entry.plugin_name,
                    "plugin view timed out",
                );
                // Dispatch timeout event to the plugin so it can clean up state
                if let Some((_, plugin)) = plugin_registry.lookup(&plugin_name_owned).await {
                    let payload = serde_json::json!({"message_id": msg_id.get()});
                    let _ =
                        invocation::dispatch_on_event_ffi(&plugin, "view_timeout", payload).await;
                }
            }
        });
    }

    /// Unregisters a plugin view, stopping interaction routing.
    pub async fn unregister(&self, msg_id: MessageId) {
        self.views.write().await.remove(&msg_id);
    }

    /// Handles a component interaction for a registered plugin view.
    ///
    /// Acknowledges the interaction, then dispatches an event to the plugin
    /// via [`invocation::dispatch_on_event_with_ctx`]. The context includes
    /// the channel ID so the plugin can edit the message via `host.edit_reply()`.
    pub async fn handle_interaction(
        &self,
        msg_id: MessageId,
        interaction: ComponentInteraction,
        data: Arc<Data>,
    ) {
        let entry = match self.views.read().await.get(&msg_id).cloned() {
            Some(e) => e,
            None => {
                tracing::debug!("interaction for unknown view");
                return;
            }
        };

        if interaction.user.id != entry.author_id {
            tracing::debug!(
                interaction.user.id = %interaction.user.id,
                expected.author.id = %entry.author_id,
                "component interaction from wrong user, ignoring",
            );
            return;
        }

        // Acknowledge the interaction so Discord doesn't show an error
        if let Err(e) = interaction
            .create_response(&self.http, CreateInteractionResponse::Acknowledge)
            .await
        {
            tracing::error!(error = %e, "failed to acknowledge component interaction");
            return;
        }

        // Build the event payload
        let mut event_payload = serde_json::json!({
            "custom_id": interaction.data.custom_id,
            "message_id": msg_id.get(),
            "user_id": interaction.user.id.get(),
            "channel_id": interaction.channel_id.get(),
        });
        if let Some(guild_id) = interaction.guild_id {
            event_payload["guild_id"] = serde_json::json!(guild_id.get());
        }
        // Include select menu values if present
        if let ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind {
            event_payload["values"] = serde_json::json!(values);
        }

        // Look up the plugin
        let plugin = match self.plugin_registry.lookup(&entry.plugin_name).await {
            Some((_, p)) => p,
            None => {
                tracing::error!(
                    plugin.name = %entry.plugin_name,
                    "plugin not found for view interaction",
                );
                return;
            }
        };

        tracing::debug!(
            plugin.name = %entry.plugin_name,
            custom_id = %interaction.data.custom_id,
            "routing component interaction to plugin",
        );

        // Create a context with channel/guild/author metadata so the plugin
        // can edit the message via host.edit_reply()
        let ctx = PoiseHostCtx::new_system_with_channel(
            data,
            self.http.clone(),
            interaction.channel_id.get(),
            interaction.guild_id.map(|g| g.get()),
            interaction.user.id.get(),
        );

        // Dispatch to the plugin's on_event handler
        if let Err(e) = invocation::dispatch_on_event_with_ctx(
            &plugin,
            "component_interaction",
            event_payload,
            ctx,
        )
        .await
        {
            tracing::error!(
                plugin.name = %entry.plugin_name,
                error = %e,
                "plugin component interaction handler failed",
            );
        }
    }
}
