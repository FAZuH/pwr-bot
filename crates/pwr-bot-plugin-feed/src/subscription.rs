use deadpool_postgres::Pool;
use pwr_bot_sdk::*;
use tokio_postgres::types::ToSql;

use crate::platform::Platforms;

pub enum SubscribeResult {
    Success { feed_id: i32, feed_name: String },
    AlreadySubscribed { feed_id: i32, feed_name: String },
}

pub enum UnsubscribeResult {
    Success { feed_id: i32, feed_name: String },
    AlreadyUnsubscribed { feed_id: i32, feed_name: String },
    NoneSubscribed { url: String },
}

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
    },
}

async fn query_json(
    pool: &Pool,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<serde_json::Value, String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;
    let rows = client.query(sql, params).await.map_err(|e| e.to_string())?;
    let json_rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let mut map = serde_json::Map::new();
            for (i, col) in row.columns().iter().enumerate() {
                let name = col.name();
                let value: serde_json::Value = row
                    .try_get::<_, serde_json::Value>(i)
                    .unwrap_or(serde_json::Value::Null);
                map.insert(name.to_string(), value);
            }
            serde_json::Value::Object(map)
        })
        .collect();
    Ok(serde_json::Value::Array(json_rows))
}

async fn db_execute(pool: &Pool, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Result<u64, String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;
    client.execute(sql, params).await.map_err(|e| e.to_string())
}

pub async fn add_subscriber(
    pool: &Pool,
    _host: &PluginHost,
    sub_type: &str,
    target_id: &str,
) -> Result<serde_json::Value, String> {
    let result = query_json(
        pool,
        "INSERT INTO subscribers (type_, target_id) VALUES ($1, $2) \
         ON CONFLICT (type_, target_id) DO NOTHING RETURNING id, type_, target_id",
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
            "SELECT id, type_, target_id FROM subscribers WHERE type_ = $1 AND target_id = $2",
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
    let _items_id = feed
        .get("items_id")
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

    let latest = platform
        .fetch_latest(&source_id)
        .await
        .map_err(|e| e.to_string())?;

    let existing = query_json(
        pool,
        "SELECT id, description, published FROM feed_items \
         WHERE feed_id = $1 ORDER BY published DESC LIMIT 1",
        &[&feed_id],
    )
    .await?;

    let existing_row = existing.as_array().and_then(|arr| arr.first());

    match existing_row {
        Some(row) => {
            let existing_desc = row
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if existing_desc == latest.title {
                return Ok(FeedUpdateResult::NoUpdate);
            }
            let old_title = Some(existing_desc.to_string());

            db_execute(
                pool,
                "INSERT INTO feed_items (feed_id, description, published) VALUES ($1, $2, $3)",
                &[&feed_id, &latest.title, &latest.published.to_rfc3339()],
            )
            .await?;

            Ok(FeedUpdateResult::Updated {
                feed_id,
                feed_name,
                old_title,
                new_title: latest.title,
                platform_name: platform.get_info().name.clone(),
                feed_item_name: platform.get_info().feed_item_name.clone(),
                logo_url: cover_url,
            })
        }
        None => {
            db_execute(
                pool,
                "INSERT INTO feed_items (feed_id, description, published) VALUES ($1, $2, $3)",
                &[&feed_id, &latest.title, &latest.published.to_rfc3339()],
            )
            .await?;

            Ok(FeedUpdateResult::Updated {
                feed_id,
                feed_name,
                old_title: None,
                new_title: latest.title,
                platform_name: platform.get_info().name.clone(),
                feed_item_name: platform.get_info().feed_item_name.clone(),
                logo_url: cover_url,
            })
        }
    }
}

pub async fn publish_update(host: &PluginHost, result: &FeedUpdateResult) {
    if let FeedUpdateResult::Updated {
        feed_id,
        feed_name,
        old_title,
        new_title,
        platform_name,
        feed_item_name,
        logo_url,
    } = result
    {
        let json = serde_json::json!({
            "feed": {
                "id": feed_id,
                "name": feed_name,
                "platform": platform_name,
                "logo_url": logo_url,
            },
            "latest": {
                "title": new_title,
                "description": new_title,
                "url": "",
            },
            "old_title": old_title,
            "item_name": feed_item_name,
        });

        let json_str = serde_json::to_string(&json).map_err(|e| e.to_string());
        if let Ok(s) = &json_str {
            let _ = unsafe { host.publish_event("feed_update", s) };
        }
    }
}
