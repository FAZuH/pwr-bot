//! Voice state event subscriber — tracks join/leave/move with raw SQL.

use std::collections::HashMap;

use chrono::DateTime;
use chrono::Utc;
use pwr_bot_sdk::*;

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

/// Parses a VoiceStateEvent JSON payload into the fields we need.
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

/// Returns true if voice tracking is explicitly disabled for a guild.
async fn is_voice_disabled(host: &PluginHost, guild_id: u64) -> Result<bool, String> {
    let result = unsafe {
        host.query_db(
            "SELECT settings->'voice'->>'enabled' AS enabled \
             FROM server_settings WHERE guild_id = $1::bigint",
            &[DbValue::I64(guild_id as i64)],
        )
        .map_err(|e| format!("is_enabled query failed: {e}"))?
    };

    let rows = match result {
        serde_json::Value::Array(arr) => arr,
        _ => return Ok(false),
    };

    match rows.first() {
        Some(row) => match row.get("enabled") {
            Some(val) if val == "false" => Ok(true), // explicitly disabled
            _ => Ok(false),                          // enabled or no setting
        },
        None => Ok(false), // no row = enabled by default
    }
}

/// Closes all active sessions for a user in a guild (orphaned session cleanup).
async fn close_orphaned_sessions(
    host: &PluginHost,
    user_id: u64,
    guild_id: u64,
) -> Result<(), String> {
    let sessions = unsafe {
        host.query_db(
            "SELECT channel_id, join_time FROM voice_sessions \
             WHERE user_id = $1::bigint AND guild_id = $2::bigint AND is_active = true",
            &[DbValue::I64(user_id as i64), DbValue::I64(guild_id as i64)],
        )
        .map_err(|e| format!("Failed to query orphaned sessions: {e}"))?
    };

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

        unsafe {
            host.execute_db(
                "UPDATE voice_sessions SET leave_time = $1::timestamptz, is_active = false \
                 WHERE user_id = $2::bigint AND channel_id = $3::bigint \
                 AND join_time = $4::timestamptz AND is_active = true",
                &[
                    DbValue::Text(now.clone()),
                    DbValue::I64(user_id as i64),
                    DbValue::I64(channel_id),
                    DbValue::Text(join_time.to_string()),
                ],
            )
            .map_err(|e| format!("Failed to close orphaned session: {e}"))?;
        }
    }

    Ok(())
}

/// Inserts a new voice session record.
async fn insert_session(
    host: &PluginHost,
    user_id: u64,
    guild_id: u64,
    channel_id: u64,
) -> Result<(), String> {
    let now = Utc::now();
    let now_rfc = now.to_rfc3339();
    unsafe {
        host.execute_db(
            "INSERT INTO voice_sessions (user_id, guild_id, channel_id, join_time, leave_time, is_active) \
             VALUES ($1::bigint, $2::bigint, $3::bigint, $4::timestamptz, $5::timestamptz, true)",
            &[
                DbValue::I64(user_id as i64),
                DbValue::I64(guild_id as i64),
                DbValue::I64(channel_id as i64),
                DbValue::Text(now_rfc.clone()),
                DbValue::Text(now_rfc),
            ],
        )
        .map_err(|e| format!("Failed to insert session: {e}"))?;
    }
    Ok(())
}

/// Handles a voice state join event.
async fn handle_join(
    active_sessions: &tokio::sync::Mutex<HashMap<String, ActiveSession>>,
    host: &PluginHost,
    parsed: &ParsedVoiceState,
    channel_id: u64,
) -> Result<(), String> {
    // Dedup: skip if already tracking this session (gateway reconnect)
    if active_sessions
        .lock()
        .await
        .contains_key(&parsed.session_id)
    {
        return Ok(());
    }

    // Close any orphaned sessions before creating a new one
    close_orphaned_sessions(host, parsed.user_id, parsed.guild_id).await?;

    // Re-check after await (race condition guard)
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

    insert_session(host, parsed.user_id, parsed.guild_id, channel_id).await
}

/// Handles a voice state leave event.
async fn handle_leave(
    active_sessions: &tokio::sync::Mutex<HashMap<String, ActiveSession>>,
    host: &PluginHost,
    parsed: &ParsedVoiceState,
) -> Result<(), String> {
    // Remove from memory
    active_sessions.lock().await.remove(&parsed.session_id);

    // Close ALL active sessions for this user (handles orphaned sessions)
    let now = Utc::now().to_rfc3339();

    let sessions = unsafe {
        host.query_db(
            "SELECT channel_id, join_time FROM voice_sessions \
             WHERE user_id = $1::bigint AND guild_id = $2::bigint AND is_active = true",
            &[
                DbValue::I64(parsed.user_id as i64),
                DbValue::I64(parsed.guild_id as i64),
            ],
        )
        .map_err(|e| format!("Failed to query active sessions: {e}"))?
    };

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

        unsafe {
            host.execute_db(
                "UPDATE voice_sessions SET leave_time = $1::timestamptz, is_active = false \
                 WHERE user_id = $2::bigint AND channel_id = $3::bigint \
                 AND join_time = $4::timestamptz AND is_active = true",
                &[
                    DbValue::Text(now.clone()),
                    DbValue::I64(parsed.user_id as i64),
                    DbValue::I64(channel_id),
                    DbValue::Text(join_time.to_string()),
                ],
            )
            .map_err(|e| format!("Failed to close session on leave: {e}"))?;
        }
    }

    Ok(())
}

/// Handles a voice state move event (channel change).
async fn handle_move(
    active_sessions: &tokio::sync::Mutex<HashMap<String, ActiveSession>>,
    host: &PluginHost,
    parsed: &ParsedVoiceState,
    new_channel_id: u64,
) -> Result<(), String> {
    // Remove old session from memory
    active_sessions.lock().await.remove(&parsed.session_id);

    // Close ALL active sessions for this user (handles orphaned sessions)
    let now = Utc::now();
    let now_rfc = now.to_rfc3339();

    let sessions = unsafe {
        host.query_db(
            "SELECT channel_id, join_time FROM voice_sessions \
             WHERE user_id = $1::bigint AND guild_id = $2::bigint AND is_active = true",
            &[
                DbValue::I64(parsed.user_id as i64),
                DbValue::I64(parsed.guild_id as i64),
            ],
        )
        .map_err(|e| format!("Failed to query sessions on move: {e}"))?
    };

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

        unsafe {
            host.execute_db(
                "UPDATE voice_sessions SET leave_time = $1::timestamptz, is_active = false \
                 WHERE user_id = $2::bigint AND channel_id = $3::bigint \
                 AND join_time = $4::timestamptz AND is_active = true",
                &[
                    DbValue::Text(now_rfc.clone()),
                    DbValue::I64(parsed.user_id as i64),
                    DbValue::I64(channel_id),
                    DbValue::Text(join_time.to_string()),
                ],
            )
            .map_err(|e| format!("Failed to close session on move: {e}"))?;
        }
    }

    // Start new session
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

    insert_session(host, parsed.user_id, parsed.guild_id, new_channel_id).await
}

/// Top-level voice state event handler for the plugin.
pub async fn handle_voice_state_event(
    active_sessions: &tokio::sync::Mutex<HashMap<String, ActiveSession>>,
    host: &PluginHost,
    payload: serde_json::Value,
) -> Result<(), String> {
    let parsed = match parse_event(&payload) {
        Some(p) => p,
        None => return Err("Failed to parse voice state event".to_string()),
    };

    // Check if voice tracking is enabled for this guild
    if is_voice_disabled(host, parsed.guild_id).await? {
        return Ok(());
    }

    match (parsed.old_channel, parsed.new_channel) {
        // User joined: old None, new Some
        (None, Some(channel_id)) => handle_join(active_sessions, host, &parsed, channel_id).await,
        // User left: old Some, new None
        (Some(_old_id), None) => handle_leave(active_sessions, host, &parsed).await,
        // User moved: both Some, different channels
        (Some(old_id), Some(new_id)) if old_id != new_id => {
            handle_move(active_sessions, host, &parsed, new_id).await
        }
        // Same channel or other state changes (mute/deafen) — ignore
        _ => Ok(()),
    }
}
