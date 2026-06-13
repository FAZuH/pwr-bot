//! Feed subscription management — raw SQL via PluginHost.

use pwr_bot_sdk::*;

use crate::platform::Platforms;

/// Result of subscribe operation.
pub enum SubscribeResult {
    Success { feed_id: i32, feed_name: String },
    AlreadySubscribed { feed_id: i32, feed_name: String },
}

/// Result of unsubscribe operation.
pub enum UnsubscribeResult {
    Success { feed_id: i32, feed_name: String },
    AlreadyUnsubscribed { feed_id: i32, feed_name: String },
    NoneSubscribed { url: String },
}

/// Result of checking a feed for updates.
pub enum FeedUpdateResult {
    NoUpdate,
    Updated {
        feed_id: i32,
        feed_name: String,
        old_title: Option<String>,
        new_title: String,
        platform_name: String,
        feed_item_name: String,
        logo_url: String,
        copyright_notice: String,
        source_url: String,
        cover_url: String,
    },
    SourceFinished,
}

/// Subscribes a user/guild to a feed URL.
pub async fn subscribe(
    host: &PluginHost,
    url: &str,
    subscriber_id: i32,
) -> Result<SubscribeResult, String> {
    // Find or create the feed
    let feed = match get_feed_by_source_url(host, url).await? {
        Some(feed) => feed,
        None => {
            
            create_feed(host, url).await?
        }
    };

    let feed_id = feed.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let feed_name = feed
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    // Check if already subscribed
    let existing = unsafe {
        host.query_db(
            "SELECT id FROM feed_subscriptions WHERE feed_id = $1 AND subscriber_id = $2",
            &[
                DbValue::I64(feed_id as i64),
                DbValue::I64(subscriber_id as i64),
            ],
        )
        .map_err(|e| format!("Failed to check subscription: {e}"))?
    };

    let rows = match existing {
        serde_json::Value::Array(arr) => arr,
        _ => vec![],
    };

    if !rows.is_empty() {
        return Ok(SubscribeResult::AlreadySubscribed {
            feed_id: feed_id as i32,
            feed_name,
        });
    }

    // Create subscription
    unsafe {
        host.execute_db(
            "INSERT INTO feed_subscriptions (feed_id, subscriber_id) VALUES ($1, $2)",
            &[
                DbValue::I64(feed_id as i64),
                DbValue::I64(subscriber_id as i64),
            ],
        )
        .map_err(|e| format!("Failed to create subscription: {e}"))?;
    }

    Ok(SubscribeResult::Success {
        feed_id: feed_id as i32,
        feed_name,
    })
}

/// Gets a feed by source URL via platform detection.
async fn get_feed_by_source_url(
    host: &PluginHost,
    source_url: &str,
) -> Result<Option<serde_json::Value>, String> {
    let result = unsafe {
        host.query_db(
            "SELECT * FROM feeds WHERE source_url = $1",
            &[DbValue::Text(source_url.to_string())],
        )
        .map_err(|e| format!("Failed to query feed: {e}"))?
    };

    match result {
        serde_json::Value::Array(arr) => Ok(arr.into_iter().next()),
        _ => Ok(None),
    }
}

/// Creates a new feed from a URL (requires platform support).
#[allow(unused_variables)]
async fn create_feed(host: &PluginHost, _source_url: &str) -> Result<serde_json::Value, String> {
    Err("Feed creation from URL not yet supported via plugin".to_string())
}

/// Unsubscribes a user/guild from a feed.
pub async fn unsubscribe(
    host: &PluginHost,
    source_url: &str,
    subscriber_id: i32,
) -> Result<UnsubscribeResult, String> {
    let feed = match get_feed_by_source_url(host, source_url).await? {
        Some(feed) => feed,
        None => {
            return Ok(UnsubscribeResult::NoneSubscribed {
                url: source_url.to_string(),
            });
        }
    };

    let feed_id = feed.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let feed_name = feed
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let affected = unsafe {
        host.execute_db(
            "DELETE FROM feed_subscriptions WHERE feed_id = $1 AND subscriber_id = $2",
            &[
                DbValue::I64(feed_id as i64),
                DbValue::I64(subscriber_id as i64),
            ],
        )
        .map_err(|e| format!("Failed to delete subscription: {e}"))?
    };

    if affected > 0 {
        Ok(UnsubscribeResult::Success {
            feed_id: feed_id as i32,
            feed_name,
        })
    } else {
        Ok(UnsubscribeResult::AlreadyUnsubscribed {
            feed_id: feed_id as i32,
            feed_name,
        })
    }
}

/// Returns paginated subscriptions for a subscriber.
pub async fn list_subscriptions(
    host: &PluginHost,
    subscriber_id: i32,
    page: u32,
    per_page: u32,
) -> Result<Vec<serde_json::Value>, String> {
    let offset = (page.saturating_sub(1)) * per_page;
    let result = unsafe {
        host.query_db(
            "SELECT f.id, f.name, f.description, f.platform_id, f.source_id, \
             f.items_id, f.source_url, f.cover_url, f.tags, \
             fi.id AS item_id, fi.description AS item_desc, fi.published AS item_published \
             FROM feed_subscriptions fs \
             JOIN feeds f ON f.id = fs.feed_id \
             LEFT JOIN feed_items fi ON fi.id = f.items_id \
             WHERE fs.subscriber_id = $1 \
             ORDER BY f.name \
             LIMIT $2 OFFSET $3",
            &[
                DbValue::I64(subscriber_id as i64),
                DbValue::I64(per_page as i64),
                DbValue::I64(offset as i64),
            ],
        )
        .map_err(|e| format!("Failed to list subscriptions: {e}"))?
    };

    match result {
        serde_json::Value::Array(arr) => Ok(arr),
        _ => Ok(vec![]),
    }
}

/// Returns total subscription count for a subscriber.
pub async fn subscription_count(host: &PluginHost, subscriber_id: i32) -> Result<u64, String> {
    let result = unsafe {
        host.query_db(
            "SELECT COUNT(*) AS count FROM feed_subscriptions WHERE subscriber_id = $1",
            &[DbValue::I64(subscriber_id as i64)],
        )
        .map_err(|e| format!("Failed to count subscriptions: {e}"))?
    };

    let rows = match result {
        serde_json::Value::Array(arr) => arr,
        _ => vec![],
    };

    Ok(rows
        .first()
        .and_then(|r| r.get("count"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as u64)
}

/// Searches subscriptions by name.
pub async fn search_subscriptions(
    host: &PluginHost,
    subscriber_id: i32,
    partial: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let pattern = format!("%{}%", partial);
    let result = unsafe {
        host.query_db(
            "SELECT f.* FROM feeds f \
             JOIN feed_subscriptions fs ON fs.feed_id = f.id \
             WHERE fs.subscriber_id = $1 AND f.name ILIKE $2 \
             LIMIT 25",
            &[DbValue::I64(subscriber_id as i64), DbValue::Text(pattern)],
        )
        .map_err(|e| format!("Failed to search subscriptions: {e}"))?
    };

    match result {
        serde_json::Value::Array(arr) => Ok(arr),
        _ => Ok(vec![]),
    }
}

/// Returns all feeds with a given tag.
pub async fn get_feeds_by_tag(
    host: &PluginHost,
    tag: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let result = unsafe {
        host.query_db(
            "SELECT * FROM feeds WHERE tags ILIKE $1",
            &[DbValue::Text(format!("%{}%", tag))],
        )
        .map_err(|e| format!("Failed to query feeds by tag: {e}"))?
    };

    match result {
        serde_json::Value::Array(arr) => Ok(arr),
        _ => Ok(vec![]),
    }
}

/// Checks a feed for updates using its platform.
pub async fn check_feed_update(
    host: &PluginHost,
    platforms: &Platforms,
    feed_row: &serde_json::Value,
) -> Result<FeedUpdateResult, String> {
    let feed_id = feed_row.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let feed_name = feed_row
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let source_url = feed_row
        .get("source_url")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let items_id = feed_row
        .get("items_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let cover_url = feed_row
        .get("cover_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Check if feed has subscribers
    let has_subs = unsafe {
        host.query_db(
            "SELECT COUNT(*) AS count FROM feed_subscriptions WHERE feed_id = $1",
            &[DbValue::I64(feed_id)],
        )
        .map_err(|e| format!("Failed to check subscribers: {e}"))?
    };

    let has_subs = match has_subs {
        serde_json::Value::Array(arr) => {
            arr.first()
                .and_then(|r| r.get("count"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                > 0
        }
        _ => false,
    };

    if !has_subs {
        return Ok(FeedUpdateResult::NoUpdate);
    }

    // Get the platform for this feed
    let platform = platforms
        .get_platform_by_source_url(source_url)
        .ok_or_else(|| format!("No platform found for URL: {source_url}"))?
        .clone();

    // Get latest known item from DB
    let old_item = unsafe {
        host.query_db(
            "SELECT id, description, published FROM feed_items WHERE feed_id = $1 ORDER BY published DESC LIMIT 1",
            &[DbValue::I64(feed_id)],
        )
        .map_err(|e| format!("Failed to query latest item: {e}"))?
    };

    let old_title = match old_item {
        serde_json::Value::Array(arr) => arr.into_iter().next().and_then(|r| {
            r.get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }),
        _ => None,
    };

    // Fetch latest from the platform API
    let new_latest = platform
        .fetch_latest(items_id)
        .await
        .map_err(|e| format!("Platform fetch failed: {e}"))?;

    // Check if version changed
    if let Some(ref old) = old_title
        && new_latest.title == *old {
            return Ok(FeedUpdateResult::NoUpdate);
        }

    // Insert new item
    unsafe {
        host.execute_db(
            "INSERT INTO feed_items (feed_id, description, published) VALUES ($1, $2, $3)",
            &[
                DbValue::I64(feed_id),
                DbValue::Text(new_latest.title.clone()),
                DbValue::Text(new_latest.published.to_rfc3339()),
            ],
        )
        .map_err(|e| format!("Failed to insert feed item: {e}"))?;
    }

    let platform_info = platform.get_info();

    Ok(FeedUpdateResult::Updated {
        feed_id: feed_id as i32,
        feed_name,
        old_title,
        new_title: new_latest.title,
        platform_name: platform_info.name.clone(),
        feed_item_name: platform_info.feed_item_name.clone(),
        logo_url: platform_info.logo_url.clone(),
        copyright_notice: platform_info.copyright_notice.clone(),
        source_url: source_url.to_string(),
        cover_url,
    })
}

/// Publishes a feed update event via the host.
pub async fn publish_update(host: &PluginHost, result: &FeedUpdateResult) {
    if let FeedUpdateResult::Updated {
        feed_id,
        feed_name,
        old_title,
        new_title,
        platform_name,
        feed_item_name,
        logo_url,
        copyright_notice,
        source_url,
        cover_url,
    } = result
    {
        let payload = serde_json::json!({
            "feed": {
                "id": feed_id,
                "name": feed_name,
                "source_url": source_url,
                "cover_url": cover_url,
            },
            "data": {
                "feed_info": {
                    "name": platform_name,
                    "feed_item_name": feed_item_name,
                    "logo_url": logo_url,
                    "copyright_notice": copyright_notice,
                },
                "old_feed_item": old_title.as_ref().map(|t| serde_json::json!({
                    "description": t,
                })),
                "new_feed_item": {
                    "description": new_title,
                },
            },
        });

        let json_str = serde_json::to_string(&payload).unwrap_or_default();
        unsafe {
            let _ = host.publish_event("feed_update", &json_str);
        }
    }
}
