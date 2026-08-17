//! Discord event fan-out: routes gateway events to the plugins that declared
//! them in their manifest `event_handlers`.
//!
//! The host reads `manifest.event_handlers` at spawn and subscribes the
//! plugin to the named Discord events ([`PluginEventRouter::subscribe`]).
//! [`BotEventHandler`](crate::bot::BotEventHandler) then fans every
//! occurrence of those events out to the subscribed plugins as one-way event
//! envelopes over the plugin wire (`Msg::Event`), so a plugin can act on
//! Discord state it never saw otherwise (e.g. `voice_state`).

use std::collections::HashMap;

use log::warn;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::plugin::PluginManager;

/// The Discord event name for `VoiceStateUpdate`, the v1 subscription.
pub const VOICE_STATE_EVENT: &str = "voice_state";

/// Routes Discord gateway events to the plugins that declared them in their
/// manifest `event_handlers`. Subscriptions are keyed by plugin name;
/// fan-out resolves the live handle through the [`PluginManager`] at
/// dispatch time, so a respawned instance keeps its subscriptions and an
/// unloaded one is skipped.
#[derive(Debug, Default)]
pub struct PluginEventRouter {
    /// Discord event name -> plugin names subscribed to it.
    subscriptions: Mutex<HashMap<String, Vec<String>>>,
}

impl PluginEventRouter {
    /// A new router with no subscriptions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribes `plugin` to the named Discord events (e.g.
    /// `voice_state`). Idempotent per (event, plugin) pair.
    pub async fn subscribe(&self, plugin: &str, events: &[String]) {
        let mut subscriptions = self.subscriptions.lock().await;
        for event in events {
            let names = subscriptions.entry(event.clone()).or_default();
            if !names.iter().any(|name| name == plugin) {
                names.push(plugin.to_string());
            }
        }
    }

    /// The plugin names subscribed to `event`, in subscription order.
    pub async fn subscribers(&self, event: &str) -> Vec<String> {
        self.subscriptions
            .lock()
            .await
            .get(event)
            .cloned()
            .unwrap_or_default()
    }

    /// Forwards `event` with `payload` to every subscribed plugin: resolves
    /// each subscriber's live handle through `manager` and pushes a one-way
    /// event envelope on a spawned task, so a slow subscriber never blocks
    /// the caller (the gateway event handler). A subscriber that is not
    /// running is skipped; a failed push is logged inside the task, never
    /// fatal.
    pub async fn fan_out(&self, manager: &PluginManager, event: &str, payload: &impl Serialize) {
        let subscribers = self.subscribers(event).await;
        if subscribers.is_empty() {
            return;
        }
        let payload = match serde_json::to_value(payload) {
            Ok(payload) => payload,
            Err(e) => {
                warn!("failed to serialize payload for event `{event}`: {e}");
                return;
            }
        };
        for name in subscribers {
            let Some(plugin) = manager.get(&name).await else {
                continue;
            };
            let payload = payload.clone();
            let event = event.to_string();
            tokio::spawn(async move {
                if let Err(e) = plugin.send_event(&event, Some(payload)).await {
                    warn!("failed to push event `{event}` to plugin {name}: {e}");
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[tokio::test]
    async fn subscribe_then_subscribers_returns_the_plugin_per_event() {
        let router = PluginEventRouter::new();
        router
            .subscribe("feed", &["voice_state".into(), "message".into()])
            .await;
        router.subscribe("hello", &["voice_state".into()]).await;

        assert_eq!(router.subscribers("voice_state").await, ["feed", "hello"]);
        assert_eq!(router.subscribers("message").await, ["feed"]);
        assert_eq!(
            router.subscribers("guild_ready").await,
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    async fn subscribing_the_same_pair_twice_is_idempotent() {
        let router = PluginEventRouter::new();
        router.subscribe("feed", &["voice_state".into()]).await;
        router.subscribe("feed", &["voice_state".into()]).await;

        assert_eq!(router.subscribers("voice_state").await, ["feed"]);
    }

    #[tokio::test]
    async fn fan_out_without_subscribers_touches_nothing() {
        let manager = Arc::new(PluginManager::new(
            None,
            crate::plugin::RespawnPolicy::default(),
        ));
        let router = PluginEventRouter::new();
        router
            .fan_out(&manager, VOICE_STATE_EVENT, &serde_json::json!({}))
            .await;
        assert_eq!(
            router.subscribers(VOICE_STATE_EVENT).await,
            Vec::<String>::new()
        );
    }
}
