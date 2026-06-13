//! Feed update subscriber — sends feed notifications to guild channels and DMs.

use pwr_bot_sdk::*;

/// Handles a `feed_update` named event.
///
/// Queries subscribers for the feed, then sends a notification to each
/// subscriber via their configured channel (guild) or DM.
pub async fn handle_feed_update(host: &PluginHost, payload: serde_json::Value) -> Result<(), String> {
    let feed_id = match extract_feed_id(&payload) {
        Some(id) => id,
        None => return Err("FeedUpdateEvent missing feed.id".to_string()),
    };

    let message = build_message(&payload);

    // Send to guild subscribers
    let guild_subs = query_subscribers(host, feed_id, "guild").await?;
    for sub in &guild_subs {
        let target_id = match sub.get("target_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };
        let guild_id: u64 = match target_id.parse() {
            Ok(id) => id,
            Err(_) => continue,
        };

        let channel_id = match resolve_guild_channel(host, guild_id).await {
            Ok(Some(id)) => id,
            Ok(None) => continue,
            Err(e) => {
                let _ = e;
                continue;
            }
        };

        unsafe {
            let _ = host.send_channel_message(channel_id, &message);
        }
    }

    // Send to DM subscribers
    let dm_subs = query_subscribers(host, feed_id, "dm").await?;
    for sub in &dm_subs {
        let target_id = match sub.get("target_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };
        let user_id: u64 = match target_id.parse() {
            Ok(id) => id,
            Err(_) => continue,
        };

        unsafe {
            let _ = host.send_dm(user_id, &message);
        }
    }

    Ok(())
}

/// Extracts the feed ID from a FeedUpdateEvent JSON payload.
fn extract_feed_id(payload: &serde_json::Value) -> Option<i64> {
    payload
        .get("feed")
        .and_then(|f| f.get("id"))
        .and_then(|v| v.as_i64())
}

/// Queries subscribers of a given type for a feed.
async fn query_subscribers(
    host: &PluginHost,
    feed_id: i64,
    type_: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let result = unsafe {
        host.query_db(
            "SELECT s.target_id \
             FROM subscribers s \
             JOIN feed_subscriptions fs ON fs.subscriber_id = s.id \
             WHERE fs.feed_id = $1 AND s.type_ = $2",
            &[DbValue::I64(feed_id), DbValue::Text(type_.into())],
        )
        .map_err(|e| format!("Failed to query subscribers: {e}"))?
    };

    match result {
        serde_json::Value::Array(arr) => Ok(arr),
        _ => Ok(vec![]),
    }
}

/// Looks up the configured notification channel for a guild.
async fn resolve_guild_channel(host: &PluginHost, guild_id: u64) -> Result<Option<u64>, String> {
    let result = unsafe {
        host.query_db(
            "SELECT settings->'feeds'->>'channel_id' AS channel_id \
             FROM server_settings WHERE guild_id = $1::bigint",
            &[DbValue::I64(guild_id as i64)],
        )
        .map_err(|e| format!("Failed to query guild settings: {e}"))?
    };

    let rows = match result {
        serde_json::Value::Array(arr) => arr,
        _ => return Ok(None),
    };

    match rows.first() {
        Some(row) => {
            let id_str = row.get("channel_id").and_then(|v| v.as_str()).unwrap_or("");
            if id_str.is_empty() {
                Ok(None)
            } else {
                match id_str.parse::<u64>() {
                    Ok(id) => Ok(Some(id)),
                    Err(_) => Ok(None),
                }
            }
        }
        None => Ok(None),
    }
}

/// Builds a text notification message from the event payload.
fn build_message(payload: &serde_json::Value) -> String {
    let feed_name = payload
        .get("feed")
        .and_then(|f| f.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown Feed");

    let source_url = payload
        .get("feed")
        .and_then(|f| f.get("source_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let feed_info = payload.get("data").and_then(|d| d.get("feed_info"));
    let item_name = feed_info
        .and_then(|i| i.get("feed_item_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("item");

    let new_item = payload
        .get("data")
        .and_then(|d| d.get("new_feed_item"));

    let item_desc = new_item
        .and_then(|i| i.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let timestamp = new_item
        .and_then(|i| i.get("published"))
        .and_then(|v| v.as_str())
        .map(|s| format_timestamp(s))
        .unwrap_or_default();

    format!(
        "### {feed_name}\n\
         **New {item_name}**: {item_desc}\n\
         {timestamp}\n\
         **[Open in browser ↗]({source_url})**"
    )
}

/// Converts an ISO 8601 timestamp to a Discord relative timestamp.
fn format_timestamp(iso: &str) -> String {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(iso) {
        format!("<t:{}>", dt.timestamp())
    } else {
        String::new()
    }
}
