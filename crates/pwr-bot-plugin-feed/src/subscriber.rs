use deadpool_postgres::Pool;
use pwr_bot_sdk::*;
use tokio_postgres::types::ToSql;

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

fn extract_feed_id(payload: &serde_json::Value) -> Option<i64> {
    payload
        .get("feed")
        .and_then(|f| f.get("id"))
        .and_then(|v| v.as_i64())
}

fn build_message(payload: &serde_json::Value) -> String {
    let feed = payload
        .get("feed")
        .and_then(|f| f.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown feed");
    let latest = payload
        .get("latest")
        .and_then(|l| l.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let item_name = payload
        .get("item_name")
        .and_then(|v| v.as_str())
        .unwrap_or("item");
    let platform = payload
        .get("feed")
        .and_then(|f| f.get("platform"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let url = payload
        .get("latest")
        .and_then(|l| l.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let logo_url = payload
        .get("feed")
        .and_then(|f| f.get("logo_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut msg = format!(
        "## **{}**\nNew **{}** on *{}*\n> {}",
        feed, item_name, platform, latest
    );
    if !url.is_empty() {
        msg.push_str(&format!("\n{}", url));
    }
    if !logo_url.is_empty() {
        msg.push_str(&format!("\n{}", logo_url));
    }
    msg
}

async fn query_subscribers(pool: &Pool, feed_id: i64, sub_type: &str) -> Vec<serde_json::Value> {
    let result = query_json(
        pool,
        "SELECT s.id, s.target_id, s.type \
         FROM subscribers s \
         JOIN feed_subscriptions fs ON fs.subscriber_id = s.id \
         WHERE fs.feed_id = $1 AND s.type = $2",
        &[&feed_id, &sub_type],
    )
    .await
    .unwrap_or(serde_json::Value::Array(vec![]));

    result.as_array().cloned().unwrap_or_default()
}

pub async fn handle_feed_update(
    pool: &Pool,
    host: &PluginHost,
    payload: serde_json::Value,
) -> Result<(), String> {
    let feed_id = match extract_feed_id(&payload) {
        Some(id) => id,
        None => return Err("FeedUpdateEvent missing feed.id".to_string()),
    };

    let message = build_message(&payload);

    let guild_subs = query_subscribers(pool, feed_id, "guild").await;
    for sub in &guild_subs {
        let target_id = match sub.get("target_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };
        let guild_id: u64 = match target_id.parse() {
            Ok(id) => id,
            Err(_) => continue,
        };
        let _ = unsafe { host.send_channel_message(guild_id, &message) };
    }

    let dm_subs = query_subscribers(pool, feed_id, "dm").await;
    for sub in &dm_subs {
        let target_id = match sub.get("target_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };
        let user_id: u64 = match target_id.parse() {
            Ok(id) => id,
            Err(_) => continue,
        };
        let _ = unsafe { host.send_dm(user_id, &message) };
    }

    Ok(())
}
