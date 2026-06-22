use chrono::Utc;
use deadpool_postgres::Pool;
use pwr_bot_plugin_util::db_execute;
use pwr_bot_plugin_util::query_json;
use pwr_bot_plugin_util::rows_to_json_vec;
use pwr_bot_sdk::*;

use crate::platform::Platforms;
use crate::platform::traits::Platform;

pub enum SubscribeResult {
    Success { feed_id: i32, feed_name: String },
    AlreadySubscribed { feed_id: i32, feed_name: String },
}

pub enum UnsubscribeResult {
    Success { feed_id: i32, feed_name: String },
    AlreadyUnsubscribed { feed_id: i32, feed_name: String },
    NoneSubscribed { url: String },
}

pub struct FeedUpdated {
    pub feed_id: i32,
    pub feed_name: String,
    pub feed_description: String,
    pub source_url: String,
    pub cover_url: String,
    pub old_title: Option<String>,
    pub old_published: Option<i64>,
    pub new_title: String,
    pub new_published: i64,
    pub platform_name: String,
    pub platform_logo_url: String,
    pub feed_item_name: String,
    pub copyright_notice: String,
}

pub enum FeedUpdateResult {
    NoUpdate,
    Updated(Box<FeedUpdated>),
}

pub async fn add_subscriber(
    pool: &Pool,
    _host: &PluginHost,
    sub_type: &str,
    target_id: &str,
) -> Result<serde_json::Value, String> {
    let result = query_json(
        pool,
        "INSERT INTO subscribers (type, target_id) VALUES ($1, $2) \
         ON CONFLICT (type, target_id) DO NOTHING RETURNING id, type, target_id",
        &[&sub_type, &target_id],
    )
    .await?;

    let rows = match result {
        serde_json::Value::Array(arr) => arr,
        _ => vec![],
    };

    match rows.first() {
        Some(row) => Ok(row.clone()),
        None => query_json(
            pool,
            "SELECT id, type, target_id FROM subscribers WHERE type = $1 AND target_id = $2",
            &[&sub_type, &target_id],
        )
        .await
        .and_then(|r| {
            r.as_array()
                .and_then(|arr| arr.first().cloned())
                .ok_or_else(|| "Failed to find subscriber after insert".to_string())
        }),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn add_feed(
    pool: &Pool,
    _host: &PluginHost,
    _url: &str,
    name: &str,
    platform_id: &str,
    source_id: &str,
    items_id: &str,
    source_url: &str,
    cover_url: &str,
    tags: &str,
    description: &str,
) -> Result<i32, String> {
    let result = query_json(
        pool,
        "INSERT INTO feeds (name, description, platform_id, source_id, items_id, source_url, cover_url, tags) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (platform_id, source_id) DO UPDATE SET name = EXCLUDED.name, \
         description = EXCLUDED.description, source_url = EXCLUDED.source_url, \
         cover_url = EXCLUDED.cover_url, tags = EXCLUDED.tags \
         RETURNING id",
        &[&name, &description, &platform_id, &source_id, &items_id, &source_url, &cover_url, &tags],
    )
    .await?;

    let id = result
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|r| r.get("id"))
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "Failed to insert feed".to_string())? as i32;

    Ok(id)
}

pub async fn subscribe(
    pool: &Pool,
    _host: &PluginHost,
    feed_id: i32,
    subscriber_id: i32,
) -> Result<SubscribeResult, String> {
    let feed = query_json(
        pool,
        "SELECT id, name FROM feeds WHERE id = $1",
        &[&feed_id],
    )
    .await?;

    let feed_row = feed
        .as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| "Feed not found".to_string())?;

    let feed_name = feed_row
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let result = query_json(
        pool,
        "INSERT INTO feed_subscriptions (feed_id, subscriber_id) VALUES ($1, $2) \
         ON CONFLICT DO NOTHING RETURNING id",
        &[&feed_id, &subscriber_id],
    )
    .await?;

    let inserted = result.as_array().map(|a| !a.is_empty()).unwrap_or(false);

    if inserted {
        Ok(SubscribeResult::Success { feed_id, feed_name })
    } else {
        Ok(SubscribeResult::AlreadySubscribed { feed_id, feed_name })
    }
}

pub async fn unsubscribe(
    pool: &Pool,
    _host: &PluginHost,
    feed_id: i32,
    subscriber_id: i32,
) -> Result<UnsubscribeResult, String> {
    let feed = query_json(
        pool,
        "SELECT id, name FROM feeds WHERE id = $1",
        &[&feed_id],
    )
    .await?;

    let feed_row = match feed.as_array().and_then(|arr| arr.first()) {
        Some(row) => row,
        None => {
            return Err("Feed not found".to_string());
        }
    };

    let feed_name = feed_row
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let deleted = db_execute(
        pool,
        "DELETE FROM feed_subscriptions WHERE feed_id = $1 AND subscriber_id = $2",
        &[&feed_id, &subscriber_id],
    )
    .await?;

    if deleted > 0 {
        Ok(UnsubscribeResult::Success { feed_id, feed_name })
    } else {
        Ok(UnsubscribeResult::AlreadyUnsubscribed { feed_id, feed_name })
    }
}

pub async fn get_feeds_by_tag(
    pool: &Pool,
    _host: &PluginHost,
    tag: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let result = query_json(
        pool,
        "SELECT id, name, description, platform_id, source_id, items_id, source_url, cover_url, tags \
         FROM feeds WHERE tags LIKE $1 ORDER BY name",
        &[&format!("%{tag}%")],
    )
    .await?;

    Ok(result.as_array().cloned().unwrap_or_default())
}

pub async fn get_feed_by_source_id(
    pool: &Pool,
    _host: &PluginHost,
    source_id: &str,
) -> Result<Option<serde_json::Value>, String> {
    let result = query_json(
        pool,
        "SELECT id, name, description, platform_id, source_id, items_id, source_url, cover_url, tags \
         FROM feeds WHERE source_id = $1",
        &[&source_id],
    )
    .await?;

    Ok(result.as_array().and_then(|arr| arr.first().cloned()))
}

/// Returns the feed for `source_id`, creating it from the platform API if it
/// does not yet exist in the database.
///
/// Mirrors the main branch's `FeedSubscriptionService::get_or_create_feed`:
/// fetches `FeedSource` from the platform, inserts a new row, fetches the
/// latest item, and inserts it as the initial `feed_items` entry.
pub async fn get_or_create_feed(
    pool: &Pool,
    _host: &PluginHost,
    platform: &(dyn Platform + Sync),
    _url: &str,
    source_id: &str,
) -> Result<serde_json::Value, String> {
    // Return existing feed if present
    if let Some(feed) = get_feed_by_source_id(pool, _host, source_id).await? {
        // Backfill: ensure at least one feed_item exists for display
        let feed_id = feed.get("id").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let items_id = feed.get("items_id").and_then(|v| v.as_str()).unwrap_or("");
        tracing::debug!(
            feed.id = feed_id,
            feed.name = ?feed.get("name").and_then(|v| v.as_str()),
            "get_or_create_feed: existing feed",
        );
        if feed_id > 0 && !items_id.is_empty() {
            let has_items = query_json(
                pool,
                "SELECT 1 FROM feed_items WHERE feed_id = $1 LIMIT 1",
                &[&feed_id],
            )
            .await
            .map(|r| r.as_array().map(|a| !a.is_empty()).unwrap_or(false))
            .unwrap_or(false);
            if !has_items {
                match platform.fetch_latest(items_id).await {
                    Ok(latest) => {
                        tracing::info!(
                            feed.id = feed_id,
                            latest.title = %latest.title,
                            "get_or_create_feed: backfilling feed_item",
                        );
                        let _ = db_execute(
                            pool,
                            "INSERT INTO feed_items (feed_id, description, published) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
                            &[&feed_id, &latest.title, &latest.published.to_rfc3339()],
                        )
                        .await;
                    }
                    Err(e) => {
                        tracing::warn!(
                            feed.id = feed_id,
                            items_id = %items_id,
                            error = %e,
                            "get_or_create_feed: fetch_latest failed, inserting placeholder",
                        );
                        eprintln!(
                            "get_or_create_feed: fetch_latest failed feed_id={feed_id} items_id={items_id}: {e}",
                        );
                        let _ = db_execute(
                            pool,
                            "INSERT INTO feed_items (feed_id, description, published) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
                            &[&feed_id, &"", &Utc::now().to_rfc3339()],
                        )
                        .await;
                    }
                }
            } else {
                tracing::debug!(
                    feed.id = feed_id,
                    "get_or_create_feed: feed already has items"
                );
            }
        }
        return Ok(feed);
    }

    // Fetch metadata from the platform API
    let feed_source = platform
        .fetch_source(source_id)
        .await
        .map_err(|e| format!("Failed to fetch feed info: {e}"))?;

    let platform_id = platform.get_id();
    let tags = &platform.get_info().tags;
    let cover_url = feed_source.image_url.as_deref().unwrap_or("");

    let feed_id = add_feed(
        pool,
        _host,
        "",
        &feed_source.name,
        platform_id,
        source_id,
        &feed_source.items_id,
        &feed_source.source_url,
        cover_url,
        tags,
        &feed_source.description,
    )
    .await?;

    // Fetch latest item and create initial version
    if let Ok(latest) = platform.fetch_latest(&feed_source.items_id).await {
        let _ = db_execute(
            pool,
            "INSERT INTO feed_items (feed_id, description, published) VALUES ($1, $2, $3)",
            &[&feed_id, &latest.title, &latest.published.to_rfc3339()],
        )
        .await;
    }

    // Return the newly-created feed
    let result = query_json(
        pool,
        "SELECT id, name, description, platform_id, source_id, items_id, source_url, cover_url, tags \
         FROM feeds WHERE id = $1",
        &[&feed_id],
    )
    .await?;

    result
        .as_array()
        .and_then(|arr| arr.first().cloned())
        .ok_or_else(|| "Failed to read back created feed".to_string())
}

/// Looks up a subscriber ID by target_id (Discord user ID as string).
pub async fn get_subscriber_by_target(pool: &Pool, target_id: &str) -> Result<Option<i32>, String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;
    let row = client
        .query_opt(
            "SELECT id FROM subscribers WHERE target_id = $1 AND type = 'dm'",
            &[&target_id],
        )
        .await
        .map_err(|e| e.to_string())?;
    drop(client);
    Ok(row.map(|r| r.get::<_, i32>(0)))
}

/// Counts the total number of subscriptions for a subscriber.
pub async fn count_subscriptions(pool: &Pool, subscriber_id: i32) -> Result<u32, String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;
    let row = client
        .query_one(
            "SELECT COUNT(*)::BIGINT FROM feed_subscriptions WHERE subscriber_id = $1",
            &[&subscriber_id],
        )
        .await
        .map_err(|e| e.to_string())?;
    let count: i64 = row.get(0);
    Ok(count as u32)
}

/// Lists a paginated slice of subscriptions for a subscriber.
///
/// Returns feed data with latest item info, ordered by feed name.
pub async fn list_paginated_subscriptions(
    pool: &Pool,
    subscriber_id: i32,
    limit: i64,
    offset: i64,
) -> Result<Vec<serde_json::Value>, String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;
    let rows = client
        .query(
            "SELECT f.id, f.name, f.description, f.platform_id, f.source_id, \
                    f.items_id, f.source_url, f.cover_url, f.tags, \
                    fi.description AS latest_title, \
                    EXTRACT(EPOCH FROM fi.published)::BIGINT AS latest_published \
             FROM feed_subscriptions fs \
             JOIN feeds f ON f.id = fs.feed_id \
             LEFT JOIN LATERAL ( \
               SELECT description, published FROM feed_items \
               WHERE feed_id = f.id ORDER BY published DESC LIMIT 1 \
             ) fi ON true \
             WHERE fs.subscriber_id = $1 \
             ORDER BY f.name \
             LIMIT $2 OFFSET $3",
            &[&subscriber_id, &limit, &offset],
        )
        .await
        .map_err(|e| e.to_string())?;
    drop(client);
    Ok(rows_to_json_vec(&rows))
}

pub async fn check_feed_update(
    pool: &Pool,
    _host: &PluginHost,
    platforms: &Platforms,
    feed: &serde_json::Value,
) -> Result<FeedUpdateResult, String> {
    let feed_id = feed.get("id").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let feed_name = feed
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();
    let feed_description = feed
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let source_url = feed
        .get("source_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let platform_id = feed
        .get("platform_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let source_id = feed
        .get("source_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let cover_url = feed
        .get("cover_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let platform = platforms
        .get_all_platforms()
        .into_iter()
        .find(|p| p.get_id() == platform_id)
        .ok_or_else(|| format!("Unknown platform '{platform_id}' for feed '{feed_name}'"))?;

    let platform_info = platform.get_info();

    let latest = platform
        .fetch_latest(&source_id)
        .await
        .map_err(|e| e.to_string())?;

    let new_published_ts = latest.published.timestamp();

    let client = pool.get().await.map_err(|e| e.to_string())?;
    let existing_row = client
        .query_opt(
            "SELECT description, EXTRACT(EPOCH FROM published)::BIGINT FROM feed_items \
             WHERE feed_id = $1 ORDER BY published DESC LIMIT 1",
            &[&feed_id],
        )
        .await
        .map_err(|e| e.to_string())?;

    drop(client);

    match existing_row {
        Some(row) => {
            let existing_desc: String = row.get(0);
            if existing_desc == latest.title {
                return Ok(FeedUpdateResult::NoUpdate);
            }
            let old_title = Some(existing_desc);
            let old_published_ts: i64 = row.get(1);

            db_execute(
                pool,
                "INSERT INTO feed_items (feed_id, description, published) VALUES ($1, $2, $3)",
                &[&feed_id, &latest.title, &latest.published.to_rfc3339()],
            )
            .await?;

            Ok(FeedUpdateResult::Updated(Box::new(FeedUpdated {
                feed_id,
                feed_name,
                feed_description,
                source_url,
                cover_url,
                old_title,
                old_published: Some(old_published_ts),
                new_title: latest.title,
                new_published: new_published_ts,
                platform_name: platform_info.name.clone(),
                platform_logo_url: platform_info.logo_url.clone(),
                feed_item_name: platform_info.feed_item_name.clone(),
                copyright_notice: platform_info.copyright_notice.clone(),
            })))
        }
        None => {
            db_execute(
                pool,
                "INSERT INTO feed_items (feed_id, description, published) VALUES ($1, $2, $3)",
                &[&feed_id, &latest.title, &latest.published.to_rfc3339()],
            )
            .await?;

            Ok(FeedUpdateResult::Updated(Box::new(FeedUpdated {
                feed_id,
                feed_name,
                feed_description,
                source_url,
                cover_url,
                old_title: None,
                old_published: None,
                new_title: latest.title,
                new_published: new_published_ts,
                platform_name: platform_info.name.clone(),
                platform_logo_url: platform_info.logo_url.clone(),
                feed_item_name: platform_info.feed_item_name.clone(),
                copyright_notice: platform_info.copyright_notice.clone(),
            })))
        }
    }
}

pub async fn publish_update(host: &PluginHost, result: &FeedUpdateResult) {
    if let FeedUpdateResult::Updated(data) = result {
        let FeedUpdated {
            feed_id,
            feed_name,
            feed_description,
            source_url,
            cover_url,
            old_title,
            old_published,
            new_title,
            new_published,
            platform_name,
            platform_logo_url,
            feed_item_name,
            copyright_notice,
        } = &**data;

        let json = serde_json::json!({
            "feed": {
                "id": feed_id,
                "name": feed_name,
                "description": feed_description,
                "platform": platform_name,
                "logo_url": platform_logo_url,
                "cover_url": cover_url,
                "source_url": source_url,
            },
            "latest": {
                "title": new_title,
                "published": new_published,
            },
            "old_title": old_title,
            "old_published": old_published,
            "item_name": feed_item_name,
            "copyright_notice": copyright_notice,
        });

        let json_str = serde_json::to_string(&json).map_err(|e| e.to_string());
        if let Ok(s) = &json_str {
            let _ = unsafe { host.publish_event("feed_update", s) };
        }
    }
}

pub async fn get_server_settings(pool: &Pool, guild_id: u64) -> Result<serde_json::Value, String> {
    let result = query_json(
        pool,
        "SELECT settings FROM server_settings WHERE guild_id = $1",
        &[&(guild_id as i64)],
    )
    .await?;
    let settings = result
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|r| r.get("settings"))
        .cloned()
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
    Ok(settings)
}

pub async fn save_feed_settings(
    pool: &Pool,
    guild_id: u64,
    feeds: &serde_json::Value,
) -> Result<(), String> {
    let mut settings = get_server_settings(pool, guild_id).await?;
    if !settings.is_object() {
        settings = serde_json::Value::Object(serde_json::Map::new());
    }
    settings["feeds"] = feeds.clone();
    let settings_str = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    db_execute(
        pool,
        "INSERT INTO server_settings (guild_id, settings) VALUES ($1, $2::jsonb) \
         ON CONFLICT (guild_id) DO UPDATE SET settings = EXCLUDED.settings",
        &[&(guild_id as i64), &settings_str],
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn save_feed_settings_preserves_voice_welcome() {
        // Verify the merge logic preserves other top-level keys.
        // This test validates the algorithm used in save_feed_settings.
        let initial = json!({"voice": {"enabled": true}, "welcome": {"enabled": false}});
        let feeds = json!({"enabled": true, "channel_id": "123"});

        let mut settings = initial.clone();
        settings["feeds"] = feeds.clone();

        assert_eq!(settings["voice"], initial["voice"]);
        assert_eq!(settings["welcome"], initial["welcome"]);
        assert_eq!(settings["feeds"], feeds);
    }

    #[test]
    fn save_feed_settings_on_empty_settings() {
        let feeds = json!({"enabled": false});
        let mut settings = serde_json::Value::Object(serde_json::Map::new());
        settings["feeds"] = feeds.clone();
        assert_eq!(settings["feeds"], feeds);
    }
}
