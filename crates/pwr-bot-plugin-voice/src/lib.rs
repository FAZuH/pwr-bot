mod subscriber;

pub mod update;

use std::collections::HashMap;

use pwr_bot_sdk::*;
use tokio::sync::Mutex;

pub struct VoicePlugin {
    active_sessions: Mutex<HashMap<String, subscriber::ActiveSession>>,
}

impl VoicePlugin {
    pub fn new() -> Self {
        Self {
            active_sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for VoicePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BotPlugin for VoicePlugin {
    fn name(&self) -> &'static str {
        "voice"
    }

    fn description(&self) -> &'static str {
        "Voice channel tracking and leaderboard"
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![CommandSpec {
            name: "vc".into(),
            description: "Voice channel tracking and leaderboard".into(),
            args: vec![],
        }]
    }

    fn settings_panels(&self) -> Vec<SettingsPanelSpec> {
        vec![SettingsPanelSpec::new("voice", "Voice")]
    }

    fn event_handlers(&self) -> Vec<EventHandlerSpec> {
        vec![EventHandlerSpec::new("voice_state".to_string())]
    }

    fn tasks(&self) -> Vec<TaskSpec> {
        vec![TaskSpec::new("voice-heartbeat", 10, "__heartbeat")]
    }

    async fn init(&self, host: &PluginHost) -> Result<(), String> {
        self.crash_recovery(host).await
    }

    async fn on_event(
        &self,
        event_name: &str,
        payload: serde_json::Value,
        host: &PluginHost,
    ) -> Result<(), String> {
        match event_name {
            "voice_state" => {
                subscriber::handle_voice_state_event(&self.active_sessions, host, payload).await
            }
            _ => Ok(()),
        }
    }

    async fn invoke(
        &self,
        command: &str,
        _args: serde_json::Value,
        host: &PluginHost,
    ) -> Result<ResponsePayload, String> {
        match command {
            "vc" | "vc leaderboard" => self.cmd_leaderboard(host).await,
            "vc stats" => self.cmd_stats(host).await,
            "__heartbeat" => self.write_heartbeat(host).await,
            _ => Err(format!("Unknown command: {command}")),
        }
    }
}

const HEARTBEAT_KEY: &str = "voice_heartbeat";

impl VoicePlugin {
    async fn crash_recovery(&self, host: &PluginHost) -> Result<(), String> {
        let result = unsafe {
            host.query_db(
                "SELECT value FROM bot_meta WHERE key = $1",
                &[DbValue::Text(HEARTBEAT_KEY.into())],
            )
            .map_err(|e| format!("Failed to read heartbeat: {e}"))?
        };

        let rows = match result {
            serde_json::Value::Array(arr) => arr,
            _ => vec![],
        };

        let last_heartbeat = match rows.first() {
            Some(row) => row
                .get("value")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            None => return Ok(()),
        };

        let last_heartbeat = match last_heartbeat {
            Some(ts) => ts,
            None => return Ok(()),
        };

        let sessions = unsafe {
            host.query_db(
                "SELECT user_id, channel_id, join_time FROM voice_sessions WHERE is_active = true",
                &[],
            )
            .map_err(|e| format!("Failed to query active sessions: {e}"))?
        };

        let rows = match sessions {
            serde_json::Value::Array(arr) => arr,
            _ => vec![],
        };

        let mut closed = 0u32;
        for row in &rows {
            let user_id = row.get("user_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let channel_id = row.get("channel_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let join_time = row.get("join_time").and_then(|v| v.as_str()).unwrap_or("");

            if user_id == 0 || channel_id == 0 || join_time.is_empty() {
                continue;
            }

            unsafe {
                host.execute_db(
                    "UPDATE voice_sessions SET leave_time = $1::timestamptz, is_active = false \
                     WHERE user_id = $2::bigint AND channel_id = $3::bigint \
                     AND join_time = $4::timestamptz AND is_active = true",
                    &[
                        DbValue::Text(last_heartbeat.clone()),
                        DbValue::I64(user_id),
                        DbValue::I64(channel_id),
                        DbValue::Text(join_time.to_string()),
                    ],
                )
                .map_err(|e| format!("Failed to close session: {e}"))?;
            }

            closed += 1;
        }

        if closed > 0 {
            // Recovery complete; host logs at info level from dispatch_init
        }

        Ok(())
    }

    async fn write_heartbeat(&self, host: &PluginHost) -> Result<ResponsePayload, String> {
        let now = chrono::Utc::now().to_rfc3339();
        unsafe {
            host.execute_db(
                "INSERT INTO bot_meta (key, value) VALUES ($1, $2) \
                 ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
                &[DbValue::Text(HEARTBEAT_KEY.into()), DbValue::Text(now)],
            )
            .map_err(|e| format!("Heartbeat write failed: {e}"))?;
        }
        Ok(ResponsePayload {
            content: None,
            ephemeral: false,
            components_json: None,
            embed_json: None,
        })
    }

    async fn cmd_leaderboard(&self, host: &PluginHost) -> Result<ResponsePayload, String> {
        unsafe { host.defer().map_err(|e| e.to_string())? };

        let guild_id = unsafe { host.guild_id() };
        if guild_id == 0 {
            return Ok(ResponsePayload {
                content: Some("This command can only be used in a server.".into()),
                ephemeral: true,
                components_json: None,
                embed_json: None,
            });
        }

        let since = chrono::Utc::now() - chrono::Duration::days(30);
        let since_ts = since.timestamp() as f64;

        let result = unsafe {
            host.query_db(
                "SELECT vs.user_id, \
                        EXTRACT(EPOCH FROM SUM(vs.leave_time - vs.join_time))::bigint AS total_duration \
                 FROM voice_sessions vs \
                 WHERE vs.guild_id = $1 \
                   AND vs.join_time >= to_timestamp($2) \
                   AND vs.join_time <= vs.leave_time \
                 GROUP BY vs.user_id \
                 ORDER BY total_duration DESC \
                 LIMIT 10",
                &[DbValue::I64(guild_id as i64), DbValue::F64(since_ts)],
            )
            .map_err(|e| format!("DB query failed: {e}"))?
        };

        let rows = match result {
            serde_json::Value::Array(arr) => arr,
            _ => vec![],
        };

        if rows.is_empty() {
            return Ok(ResponsePayload {
                content: Some("No voice activity data available for the last 30 days.".into()),
                ephemeral: false,
                components_json: None,
                embed_json: None,
            });
        }

        let mut lines: Vec<String> = vec!["## Voice Leaderboard (Last 30 Days)".to_string()];
        for (i, row) in rows.iter().enumerate() {
            let user_id = row.get("user_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let total_secs = row
                .get("total_duration")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let hours = total_secs / 3600;
            let mins = (total_secs % 3600) / 60;
            let medal = match i {
                0 => "🥇",
                1 => "🥈",
                2 => "🥉",
                _ => "  ",
            };

            lines.push(format!(
                "{medal} {}. <@{user_id}> — **{hours}h {mins}m**",
                i + 1
            ));
        }

        Ok(ResponsePayload {
            content: Some(lines.join("\n")),
            ephemeral: false,
            components_json: None,
            embed_json: None,
        })
    }

    async fn cmd_stats(&self, host: &PluginHost) -> Result<ResponsePayload, String> {
        unsafe { host.defer().map_err(|e| e.to_string())? };

        let guild_id = unsafe { host.guild_id() };
        if guild_id == 0 {
            return Ok(ResponsePayload {
                content: Some("This command can only be used in a server.".into()),
                ephemeral: true,
                components_json: None,
                embed_json: None,
            });
        }

        let since = chrono::Utc::now() - chrono::Duration::days(7);
        let since_ts = since.timestamp() as f64;

        let result = unsafe {
            host.query_db(
                "SELECT COUNT(DISTINCT vs.user_id) AS active_users, \
                        EXTRACT(EPOCH FROM SUM(vs.leave_time - vs.join_time))::bigint AS total_seconds \
                 FROM voice_sessions vs \
                 WHERE vs.guild_id = $1 \
                   AND vs.join_time >= to_timestamp($2) \
                   AND vs.join_time <= vs.leave_time",
                &[DbValue::I64(guild_id as i64), DbValue::F64(since_ts)],
            )
            .map_err(|e| format!("DB query failed: {e}"))?
        };

        let rows = match result {
            serde_json::Value::Array(arr) => arr,
            _ => vec![],
        };

        let stats = rows.first();

        let active_users = stats
            .and_then(|r| r.get("active_users"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let total_seconds = stats
            .and_then(|r| r.get("total_seconds"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let avg_seconds = if active_users > 0 {
            total_seconds / active_users
        } else {
            0
        };

        let total_hours = total_seconds / 3600;
        let avg_hours = avg_seconds / 3600;
        let avg_mins = (avg_seconds % 3600) / 60;

        let content = format!(
            "## Voice Stats (Last 7 Days)\n\
             - **Active Users:** {active_users}\n\
             - **Total Time:** {total_hours}h\n\
             - **Average per User:** {avg_hours}h {avg_mins}m"
        );

        Ok(ResponsePayload {
            content: Some(content),
            ephemeral: false,
            components_json: None,
            embed_json: None,
        })
    }
}
