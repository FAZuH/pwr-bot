use std::collections::HashMap;

use chrono::DateTime;
use chrono::Utc;
use deadpool_postgres::Pool;
use tokio_postgres::types::ToSql;

/// In-memory session tracking keyed by Discord session_id.
#[derive(Clone, Debug)]
pub struct ActiveSession {
    pub user_id: u64,
    pub guild_id: u64,
    pub channel_id: u64,
    pub join_time: DateTime<Utc>,
}

/// Lightweight parsed voice state for the plugin.
struct ParsedVoiceState {
    user_id: u64,
    guild_id: u64,
    new_channel: Option<u64>,
    old_channel: Option<u64>,
    session_id: String,
}

fn rows_to_json(rows: &[tokio_postgres::Row]) -> serde_json::Value {
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
    serde_json::Value::Array(json_rows)
}

async fn query_json(
    pool: &Pool,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<serde_json::Value, String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;
    let rows = client.query(sql, params).await.map_err(|e| e.to_string())?;
    Ok(rows_to_json(&rows))
}

async fn db_execute(pool: &Pool, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Result<u64, String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;
    client.execute(sql, params).await.map_err(|e| e.to_string())
}

fn parse_event(payload: &serde_json::Value) -> Option<ParsedVoiceState> {
    let new = payload.get("new")?;

    let user_id = new.get("user_id")?.as_str()?.parse::<u64>().ok()?;
    let guild_id = new.get("guild_id")?.as_str()?.parse::<u64>().ok()?;
    let new_channel = new
        .get("channel_id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok());
    let session_id = new.get("session_id")?.as_str()?.to_string();

    let old_channel = payload
        .get("old")
        .and_then(|o| o.get("channel_id"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok());

    Some(ParsedVoiceState {
        user_id,
        guild_id,
        new_channel,
        old_channel,
        session_id,
    })
}

async fn is_voice_disabled(pool: &Pool, guild_id: u64) -> Result<bool, String> {
    let result = query_json(
        pool,
        "SELECT settings->'voice'->>'enabled' AS enabled \
         FROM server_settings WHERE guild_id = $1::bigint",
        &[&(guild_id as i64)],
    )
    .await?;

    let rows = match result {
        serde_json::Value::Array(arr) => arr,
        _ => return Ok(false),
    };

    match rows.first() {
        Some(row) => match row.get("enabled") {
            Some(val) if val == "false" => Ok(true),
            _ => Ok(false),
        },
        None => Ok(false),
    }
}

async fn close_orphaned_sessions(pool: &Pool, user_id: u64, guild_id: u64) -> Result<(), String> {
    let sessions = query_json(
        pool,
        "SELECT channel_id, join_time FROM voice_sessions \
         WHERE user_id = $1::bigint AND guild_id = $2::bigint AND is_active = true",
        &[&(user_id as i64), &(guild_id as i64)],
    )
    .await?;

    let now = Utc::now().to_rfc3339();
    let rows = match sessions {
        serde_json::Value::Array(arr) => arr,
        _ => vec![],
    };

    for row in &rows {
        let channel_id = row.get("channel_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let join_time = row.get("join_time").and_then(|v| v.as_str()).unwrap_or("");

        if channel_id == 0 || join_time.is_empty() {
            continue;
        }

        db_execute(
            pool,
            "UPDATE voice_sessions SET leave_time = $1::timestamptz, is_active = false \
             WHERE user_id = $2::bigint AND channel_id = $3::bigint \
             AND join_time = $4::timestamptz AND is_active = true",
            &[&now, &(user_id as i64), &channel_id, &join_time.to_string()],
        )
        .await?;
    }

    Ok(())
}

async fn insert_session(
    pool: &Pool,
    user_id: u64,
    guild_id: u64,
    channel_id: u64,
) -> Result<(), String> {
    let now = Utc::now();
    let now_rfc = now.to_rfc3339();
    db_execute(
        pool,
        "INSERT INTO voice_sessions (user_id, guild_id, channel_id, join_time, leave_time, is_active) \
         VALUES ($1::bigint, $2::bigint, $3::bigint, $4::timestamptz, $5::timestamptz, true)",
        &[
            &(user_id as i64),
            &(guild_id as i64),
            &(channel_id as i64),
            &now_rfc,
            &now_rfc,
        ],
    )
    .await?;
    Ok(())
}

async fn handle_join(
    active_sessions: &tokio::sync::Mutex<HashMap<String, ActiveSession>>,
    pool: &Pool,
    parsed: &ParsedVoiceState,
    channel_id: u64,
) -> Result<(), String> {
    if active_sessions
        .lock()
        .await
        .contains_key(&parsed.session_id)
    {
        return Ok(());
    }

    close_orphaned_sessions(pool, parsed.user_id, parsed.guild_id).await?;

    if active_sessions
        .lock()
        .await
        .contains_key(&parsed.session_id)
    {
        return Ok(());
    }

    let join_time = Utc::now();
    let session = ActiveSession {
        user_id: parsed.user_id,
        guild_id: parsed.guild_id,
        channel_id,
        join_time,
    };

    active_sessions
        .lock()
        .await
        .insert(parsed.session_id.clone(), session);

    insert_session(pool, parsed.user_id, parsed.guild_id, channel_id).await
}

async fn handle_leave(
    active_sessions: &tokio::sync::Mutex<HashMap<String, ActiveSession>>,
    pool: &Pool,
    parsed: &ParsedVoiceState,
) -> Result<(), String> {
    active_sessions.lock().await.remove(&parsed.session_id);

    let now = Utc::now().to_rfc3339();

    let sessions = query_json(
        pool,
        "SELECT channel_id, join_time FROM voice_sessions \
         WHERE user_id = $1::bigint AND guild_id = $2::bigint AND is_active = true",
        &[&(parsed.user_id as i64), &(parsed.guild_id as i64)],
    )
    .await?;

    let rows = match sessions {
        serde_json::Value::Array(arr) => arr,
        _ => vec![],
    };

    for row in &rows {
        let channel_id = row.get("channel_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let join_time = row.get("join_time").and_then(|v| v.as_str()).unwrap_or("");

        if channel_id == 0 || join_time.is_empty() {
            continue;
        }

        db_execute(
            pool,
            "UPDATE voice_sessions SET leave_time = $1::timestamptz, is_active = false \
             WHERE user_id = $2::bigint AND channel_id = $3::bigint \
             AND join_time = $4::timestamptz AND is_active = true",
            &[
                &now,
                &(parsed.user_id as i64),
                &channel_id,
                &join_time.to_string(),
            ],
        )
        .await?;
    }

    Ok(())
}

async fn handle_move(
    active_sessions: &tokio::sync::Mutex<HashMap<String, ActiveSession>>,
    pool: &Pool,
    parsed: &ParsedVoiceState,
    new_channel_id: u64,
) -> Result<(), String> {
    active_sessions.lock().await.remove(&parsed.session_id);

    let now = Utc::now();
    let now_rfc = now.to_rfc3339();

    let sessions = query_json(
        pool,
        "SELECT channel_id, join_time FROM voice_sessions \
         WHERE user_id = $1::bigint AND guild_id = $2::bigint AND is_active = true",
        &[&(parsed.user_id as i64), &(parsed.guild_id as i64)],
    )
    .await?;

    let rows = match sessions {
        serde_json::Value::Array(arr) => arr,
        _ => vec![],
    };

    for row in &rows {
        let channel_id = row.get("channel_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let join_time = row.get("join_time").and_then(|v| v.as_str()).unwrap_or("");

        if channel_id == 0 || join_time.is_empty() {
            continue;
        }

        db_execute(
            pool,
            "UPDATE voice_sessions SET leave_time = $1::timestamptz, is_active = false \
             WHERE user_id = $2::bigint AND channel_id = $3::bigint \
             AND join_time = $4::timestamptz AND is_active = true",
            &[
                &now_rfc,
                &(parsed.user_id as i64),
                &channel_id,
                &join_time.to_string(),
            ],
        )
        .await?;
    }

    let session = ActiveSession {
        user_id: parsed.user_id,
        guild_id: parsed.guild_id,
        channel_id: new_channel_id,
        join_time: now,
    };

    active_sessions
        .lock()
        .await
        .insert(parsed.session_id.clone(), session);

    insert_session(pool, parsed.user_id, parsed.guild_id, new_channel_id).await
}

pub async fn handle_voice_state_event(
    active_sessions: &tokio::sync::Mutex<HashMap<String, ActiveSession>>,
    pool: &tokio::sync::Mutex<Option<Pool>>,
    payload: serde_json::Value,
) -> Result<(), String> {
    let parsed = match parse_event(&payload) {
        Some(p) => p,
        None => return Err("Failed to parse voice state event".to_string()),
    };

    let pool = pool
        .lock()
        .await
        .as_ref()
        .ok_or("DB not initialized")?
        .clone();

    if is_voice_disabled(&pool, parsed.guild_id).await? {
        return Ok(());
    }

    match (parsed.old_channel, parsed.new_channel) {
        (None, Some(channel_id)) => handle_join(active_sessions, &pool, &parsed, channel_id).await,
        (Some(_old_id), None) => handle_leave(active_sessions, &pool, &parsed).await,
        (Some(old_id), Some(new_id)) if old_id != new_id => {
            handle_move(active_sessions, &pool, &parsed, new_id).await
        }
        _ => Ok(()),
    }
}
