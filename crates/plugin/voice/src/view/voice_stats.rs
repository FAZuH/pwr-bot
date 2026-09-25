//! Rendering for the voice statistics view.
//!
//! The view renders the summary, chart attachment, time-range controls, and
//! statistic controls from [`VoiceStatsModel`]. Fetching and image rendering
//! remain adapter effects; this module only turns the model into a Discord
//! Components V2 payload.

use std::collections::HashMap;

use chrono::DateTime;
use chrono::Utc;
use contribution_grid::ContributionGraph;
use contribution_grid::builtins::Strategy;
use contribution_grid::builtins::Theme;
use pwr_ext::view_support::MessageFlags;
use serde_json::Value;
use serde_json::json;

use crate::GuildDailyStats;
use crate::GuildStatType;
use crate::TimeRange;
use crate::VoiceDailyActivity;
use crate::VoiceSessionsEntity;
use crate::VoiceStatsTimeRange;
use crate::stats::chart::generate_line_chart;
use crate::update::voice_stats::VoiceStatsModel;
use crate::update::voice_stats::VoiceStatsMsg;
use crate::utils::format_duration;

pub const IMAGE_FILENAME: &str = "voice_stats.png";
pub const TOGGLE_MODE: &str = "voice-stats:toggle";
pub const TIME_YEARLY: &str = "voice-stats:yearly";
pub const TIME_MONTHLY: &str = "voice-stats:monthly";
pub const TIME_WEEKLY: &str = "voice-stats:weekly";
pub const TIME_HOURLY: &str = "voice-stats:hourly";
pub const STAT_UNIQUE: &str = "voice-stats:unique";
pub const STAT_TOTAL: &str = "voice-stats:total";
pub const STAT_AVERAGE: &str = "voice-stats:average";
pub const SELECT_USER: &str = "voice-stats:user";

const FLAG: u64 = MessageFlags::IS_COMPONENTS_V2.bits() as u64;

pub fn components(model: &VoiceStatsModel, now: DateTime<Utc>) -> Value {
    let toggle_label = if model.is_user_stats() {
        "Show server stats"
    } else {
        "Show user stats"
    };
    let mut container = vec![json!({
        "type": 10,
        "content": format_stats_summary(model, now),
    })];
    container.push(json!({ "type": 14, "divider": true }));
    if model.user_activity().is_empty() && model.guild_stats().is_empty() {
        container.push(json!({
            "type": 10,
            "content": concat!(
                "No voice activity recorded for this time range.\n\n",
                "Join a **voice channel** to start tracking!"
            )
        }));
    } else {
        container.push(json!({
            "type": 12,
            "items": [{ "media": { "url": format!("attachment://{IMAGE_FILENAME}") } }]
        }));
    }
    container.push(json!({
        "type": 1,
        "components": [{
            "type": 2,
            "custom_id": TOGGLE_MODE,
            "label": toggle_label,
            "style": 1,
            "disabled": false
        }]
    }));

    let mut components = vec![json!({ "type": 17, "components": container })];
    components.push(json!({
        "type": 1,
        "components": [
            button(TIME_YEARLY, "Yearly", model.time_range() == VoiceStatsTimeRange::Yearly),
            button(TIME_MONTHLY, "Monthly", model.time_range() == VoiceStatsTimeRange::Monthly),
            button(TIME_WEEKLY, "Weekly", model.time_range() == VoiceStatsTimeRange::Weekly),
            button(TIME_HOURLY, "Hourly", model.time_range() == VoiceStatsTimeRange::Hourly),
        ]
    }));
    let mut stats = Vec::new();
    if !model.is_user_stats() {
        stats.push(button(
            STAT_UNIQUE,
            "Unique Users",
            model.stat_type() == GuildStatType::ActiveUserCount,
        ));
    }
    stats.push(button(
        STAT_TOTAL,
        "Total Time",
        model.stat_type() == GuildStatType::TotalTime,
    ));
    stats.push(button(
        STAT_AVERAGE,
        "Average Time",
        model.stat_type() == GuildStatType::AverageTime,
    ));
    components.push(json!({ "type": 1, "components": stats }));
    if model.is_user_stats()
        && let Some(user_id) = model.user_id()
    {
        components.push(json!({
            "type": 1,
            "components": [{
                "type": 5,
                "custom_id": SELECT_USER,
                "default_values": [{ "id": user_id, "type": "user" }]
            }]
        }));
    }
    json!({ "flags": FLAG, "components": components })
}

fn button(id: &str, label: &str, selected: bool) -> Value {
    json!({
        "type": 2,
        "custom_id": id,
        "label": label,
        "style": if selected { 1 } else { 2 },
        "disabled": false
    })
}

pub fn translate(
    custom_id: &str,
    values: &[String],
    model: &VoiceStatsModel,
) -> Result<VoiceStatsMsg, String> {
    let message = match custom_id {
        TOGGLE_MODE => VoiceStatsMsg::ToggleDataMode,
        TIME_YEARLY => VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Yearly),
        TIME_MONTHLY => VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Monthly),
        TIME_WEEKLY => VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Weekly),
        TIME_HOURLY => VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Hourly),
        STAT_UNIQUE => VoiceStatsMsg::ChangeStatType(GuildStatType::ActiveUserCount),
        STAT_TOTAL => VoiceStatsMsg::ChangeStatType(GuildStatType::TotalTime),
        STAT_AVERAGE => VoiceStatsMsg::ChangeStatType(GuildStatType::AverageTime),
        SELECT_USER => {
            let user_id = values.first().and_then(|value| value.parse().ok());
            VoiceStatsMsg::SetUser(user_id)
        }
        other => return Err(format!("unknown voice stats action `{other}`")),
    };
    let _ = model;
    Ok(message)
}

fn format_stats_summary(model: &VoiceStatsModel, now: DateTime<Utc>) -> String {
    let (since, until) = model.time_range().to_range(now);
    let time_range = format!(
        "-# Time Range: **{}** — <t:{}:f> to <t:{}:R>",
        model.time_range().display_name(),
        since.timestamp(),
        until.timestamp()
    );
    if model.is_user_stats() {
        return format!(
            concat!(
                "### Voice Stats\n{}\n\n**User:** {}\n",
                "**Total Time:** {}\n**Average Daily:** {}\n",
                "**Current Streak:** {} day(s)"
            ),
            time_range,
            model.display_name(),
            format_duration(model.total_time()),
            format_duration(model.average_daily_time()),
            model.current_streak(now)
        );
    }
    let (first_label, first_value, second_label, second_value) = match model.stat_type() {
        GuildStatType::AverageTime => {
            let peak = model.guild_stats().iter().max_by_key(|stat| stat.value);
            let value = peak
                .map(|stat| format_duration(stat.value))
                .unwrap_or_else(|| "None".into());
            let day = peak
                .map(|stat| stat.day)
                .unwrap_or_else(|| now.date_naive());
            (
                "Peak Time",
                value.clone(),
                "Most Active",
                format!(
                    " {value} on <t:{}:d>",
                    day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()
                ),
            )
        }
        GuildStatType::ActiveUserCount => {
            let users = if model.guild_stats().is_empty() {
                0
            } else {
                (model.total_active_users() as f64 / model.guild_stats().len() as f64).ceil() as i64
            };
            let peak = model.guild_stats().iter().max_by_key(|stat| stat.value);
            let value = peak
                .map(|stat| stat.value.to_string())
                .unwrap_or_else(|| "None".into());
            let day = peak
                .map(|stat| stat.day)
                .unwrap_or_else(|| now.date_naive());
            (
                "Avg Daily Users",
                users.to_string(),
                "Most Active",
                format!(
                    " {value} on <t:{}:d>",
                    day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()
                ),
            )
        }
        GuildStatType::TotalTime => {
            let peak = model.guild_stats().iter().max_by_key(|stat| stat.value);
            let value = peak
                .map(|stat| format_duration(stat.value))
                .unwrap_or_else(|| "None".into());
            let day = peak
                .map(|stat| stat.day)
                .unwrap_or_else(|| now.date_naive());
            (
                "Peak Total Time",
                value.clone(),
                "Most Active",
                format!(
                    " {value} on <t:{}:d>",
                    day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()
                ),
            )
        }
    };
    format!(
        "### Voice Stats\n{}\n\n**Server:** {}\n**{}:** {}\n**{}:**{}",
        time_range,
        model.guild_name(),
        first_label,
        first_value,
        second_label,
        second_value
    )
}

pub fn generate_image(
    now: DateTime<Utc>,
    time_range: VoiceStatsTimeRange,
    stat_type: GuildStatType,
    is_user: bool,
    raw_sessions: &[VoiceSessionsEntity],
    user_activity: &[VoiceDailyActivity],
    guild_stats: &[GuildDailyStats],
) -> anyhow::Result<Vec<u8>> {
    if time_range != VoiceStatsTimeRange::Yearly {
        return generate_line_chart(now, raw_sessions, time_range, stat_type, is_user);
    }
    let (since, _) = time_range.to_range(now);
    let today = now.date_naive();
    let mut data = HashMap::new();
    if is_user {
        for activity in user_activity {
            data.insert(activity.day, (activity.total_seconds / 60).max(1) as u32);
        }
    } else {
        for stat in guild_stats {
            let value = if stat_type == GuildStatType::ActiveUserCount {
                stat.value as u32
            } else {
                (stat.value / 60).max(1) as u32
            };
            data.insert(stat.day, value);
        }
    }
    let image = ContributionGraph::new()
        .with_data(data)
        .start_date(since.date_naive())
        .end_date(today)
        .theme(Theme::github(Strategy::linear()))
        .generate();
    let mut bytes = Vec::new();
    image.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::update::voice_stats::VoiceStatsData;

    fn test_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
            .expect("fixed test time")
            .with_timezone(&Utc)
    }

    fn normalize_timestamps(value: &mut Value) {
        match value {
            Value::String(text) => {
                let mut normalized = String::with_capacity(text.len());
                let mut rest = text.as_str();
                while let Some(start) = rest.find("<t:") {
                    normalized.push_str(&rest[..start]);
                    let after_marker = &rest[start + 3..];
                    if let Some(end) = after_marker.find(':') {
                        let timestamp = &after_marker[..end];
                        if !timestamp.is_empty()
                            && timestamp
                                .chars()
                                .all(|character| character.is_ascii_digit())
                        {
                            normalized.push_str("<t:TS:");
                            rest = &after_marker[end + 1..];
                            continue;
                        }
                    }
                    normalized.push_str("<t:");
                    rest = after_marker;
                }
                normalized.push_str(rest);
                *text = normalized;
            }
            Value::Array(values) => {
                for value in values {
                    normalize_timestamps(value);
                }
            }
            Value::Object(values) => {
                for value in values.values_mut() {
                    normalize_timestamps(value);
                }
            }
            _ => {}
        }
    }

    fn normalized_components(model: &VoiceStatsModel) -> Value {
        let mut value = components(model, test_now());
        normalize_timestamps(&mut value);
        value["components"].clone()
    }

    #[test]
    fn voice_stats_render_snapshot_guild_state() {
        let model = VoiceStatsModel::new(
            VoiceStatsTimeRange::Monthly,
            GuildStatType::TotalTime,
            None,
            1,
            VoiceStatsData {
                guild_name: "Test Server".to_string(),
                user_activity: vec![],
                guild_stats: vec![
                    GuildDailyStats {
                        day: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                        value: 3600,
                    },
                    GuildDailyStats {
                        day: NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
                        value: 7200,
                    },
                ],
                raw_sessions: vec![],
                target_user_name: None,
            },
            None,
        );
        assert_eq!(
            normalized_components(&model),
            json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "### Voice Stats\n-# Time Range: **Monthly** — <t:TS:f> to <t:TS:R>\n\n**Server:** Test Server\n**Peak Total Time:** 2h\n**Most Active:** 2h on <t:TS:d>"
                        },
                        { "type": 14, "divider": true },
                        {
                            "type": 12,
                            "items": [ { "media": { "url": "attachment://voice_stats.png" } } ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": TOGGLE_MODE,
                                    "disabled": false,
                                    "label": "Show user stats",
                                    "style": 1
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        { "type": 2, "custom_id": TIME_YEARLY, "disabled": false, "label": "Yearly", "style": 2 },
                        { "type": 2, "custom_id": TIME_MONTHLY, "disabled": false, "label": "Monthly", "style": 1 },
                        { "type": 2, "custom_id": TIME_WEEKLY, "disabled": false, "label": "Weekly", "style": 2 },
                        { "type": 2, "custom_id": TIME_HOURLY, "disabled": false, "label": "Hourly", "style": 2 }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        { "type": 2, "custom_id": STAT_UNIQUE, "disabled": false, "label": "Unique Users", "style": 2 },
                        { "type": 2, "custom_id": STAT_TOTAL, "disabled": false, "label": "Total Time", "style": 1 },
                        { "type": 2, "custom_id": STAT_AVERAGE, "disabled": false, "label": "Average Time", "style": 2 }
                    ]
                }
            ])
        );
    }

    #[test]
    fn voice_stats_render_snapshot_user_state() {
        let model = VoiceStatsModel::new(
            VoiceStatsTimeRange::Monthly,
            GuildStatType::TotalTime,
            Some(123_456_789),
            1,
            VoiceStatsData {
                guild_name: "Test Server".to_string(),
                user_activity: vec![],
                guild_stats: vec![],
                raw_sessions: vec![],
                target_user_name: Some("tester".to_string()),
            },
            None,
        );
        assert_eq!(
            normalized_components(&model),
            json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "### Voice Stats\n-# Time Range: **Monthly** — <t:TS:f> to <t:TS:R>\n\n**User:** tester\n**Total Time:** 0s\n**Average Daily:** 0s\n**Current Streak:** 0 day(s)"
                        },
                        { "type": 14, "divider": true },
                        {
                            "type": 10,
                            "content": concat!(
                "No voice activity recorded for this time range.\n\n",
                "Join a **voice channel** to start tracking!"
            )
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": TOGGLE_MODE,
                                    "disabled": false,
                                    "label": "Show server stats",
                                    "style": 1
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        { "type": 2, "custom_id": TIME_YEARLY, "disabled": false, "label": "Yearly", "style": 2 },
                        { "type": 2, "custom_id": TIME_MONTHLY, "disabled": false, "label": "Monthly", "style": 1 },
                        { "type": 2, "custom_id": TIME_WEEKLY, "disabled": false, "label": "Weekly", "style": 2 },
                        { "type": 2, "custom_id": TIME_HOURLY, "disabled": false, "label": "Hourly", "style": 2 }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        { "type": 2, "custom_id": STAT_TOTAL, "disabled": false, "label": "Total Time", "style": 1 },
                        { "type": 2, "custom_id": STAT_AVERAGE, "disabled": false, "label": "Average Time", "style": 2 }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 5,
                            "custom_id": SELECT_USER,
                            "default_values": [ { "id": 123456789, "type": "user" } ]
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn toggle_data_mode_button_switches_to_user_stats_and_requests_a_query() {
        let mut model = VoiceStatsModel::new(
            VoiceStatsTimeRange::Monthly,
            GuildStatType::AverageTime,
            None,
            1,
            VoiceStatsData {
                guild_name: "Test Server".to_string(),
                user_activity: vec![],
                guild_stats: vec![],
                raw_sessions: vec![],
                target_user_name: None,
            },
            None,
        );
        assert!(!model.is_user_stats());
        let message = translate(TOGGLE_MODE, &[], &model).expect("translate toggle");
        assert_eq!(message, VoiceStatsMsg::ToggleDataMode);

        let effects = crate::update::voice_stats::update(message, &mut model, test_now());
        assert!(model.is_user_stats());
        assert_eq!(
            effects,
            vec![crate::update::voice_stats::VoiceStatsEffect::QueryStats {
                now: test_now(),
                time_range: VoiceStatsTimeRange::Monthly,
                stat_type: GuildStatType::AverageTime,
                user_id: Some(1),
            }]
        );
        let rendered = components(&model, test_now());
        assert!(rendered.to_string().contains("Show server stats"));
        assert!(!rendered.to_string().contains("Unique Users"));
        assert!(rendered.to_string().contains(SELECT_USER));
    }
}
