//! Rendering for the voice leaderboard view.
//!
//! The view renders the leaderboard image, range selector, partner controls,
//! and pagination from [`VoiceLeaderboardModel`]. Fetching entries and page
//! images remain adapter effects.

use pwr_ext::view_support::MessageFlags;
use pwr_plugin_support::pagination::PaginationAction;
use serde_json::Value;
use serde_json::json;

use crate::TimeRange;
use crate::VoiceLeaderboardTimeRange;
use crate::update::voice_leaderboard::VoiceLeaderboardModel;
use crate::update::voice_leaderboard::VoiceLeaderboardMsg;
use crate::utils::format_duration;

pub const IMAGE_FILENAME: &str = "voice_leaderboard.jpg";
pub const TIME_RANGE: &str = "voice-leaderboard:time-range";
pub const TOGGLE_MODE: &str = "voice-leaderboard:toggle";
pub const SELECT_USER: &str = "voice-leaderboard:user";
pub const FIRST: &str = "voice-leaderboard:first";
pub const PREVIOUS: &str = "voice-leaderboard:previous";
pub const NEXT: &str = "voice-leaderboard:next";
pub const LAST: &str = "voice-leaderboard:last";

const FLAG: u64 = MessageFlags::IS_COMPONENTS_V2.bits() as u64;

pub fn components(model: &VoiceLeaderboardModel) -> Value {
    let title = if model.is_partner_mode() {
        format!(
            "### {} Voice Partners",
            model.target_user_name().unwrap_or("Your")
        )
    } else {
        "### Voice Leaderboard".to_string()
    };
    let mut container = vec![json!({ "type": 10, "content": title })];
    if let Some(rank) = model.user_rank() {
        let duration = model
            .user_duration()
            .map(format_duration)
            .unwrap_or_else(|| "unknown".into());
        container.push(json!({
            "type": 10,
            "content": format!(
                concat!(
                    "\nYou are ranked **#{rank}** on this server with ",
                    "**{duration}** of voice activity."
                ),
                rank = rank,
                duration = duration
            )
        }));
    } else if !model.target_is_author() {
        container.push(json!({
            "type": 10,
            "content": "\nYou are not on the leaderboard for this time range."
        }));
    }
    let (since, until) = model.time_range().to_range(model.now);
    container.push(json!({
        "type": 10,
        "content": format!(
            "\n-# Time Range: **{}** — <t:{}:f> to <t:{}:R>",
            model.time_range().display_name(), since.timestamp(), until.timestamp()
        )
    }));
    container.push(json!({ "type": 14, "divider": true }));
    if model.is_empty() {
        container.push(json!({
            "type": 10,
            "content": concat!(
                "No voice activity recorded yet at this time range.\n\n",
                "Join a **voice channel** to start tracking!"
            )
        }));
    } else {
        container.push(json!({
            "type": 12,
            "items": [{ "media": { "url": format!("attachment://{IMAGE_FILENAME}") } }]
        }));
    }
    let toggle_label = if model.is_partner_mode() {
        "Show Server Leaderboard"
    } else {
        "Show Voice Partners"
    };
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

    let mut components = vec![
        json!({ "type": 17, "components": container }),
        json!({
            "type": 1,
            "components": [{
                "type": 3,
                "custom_id": TIME_RANGE,
                "options": [
                    option("Past 24 hours"),
                    option("Past 72 hours"),
                    option("Past 7 days"),
                    option("Past 14 days"),
                    option("This month"),
                    option("This year"),
                    option("All time")
                ],
                "placeholder": "Select time range"
            }]
        }),
    ];
    if model.is_partner_mode() {
        let default_values = model
            .target_user_id()
            .map(|id| json!([{ "id": id, "type": "user" }]))
            .unwrap_or_else(|| json!([]));
        components.push(json!({
            "type": 1,
            "components": [{
                "type": 5,
                "custom_id": SELECT_USER,
                "default_values": default_values,
                "placeholder": "Select an user to view their voice partners"
            }]
        }));
    }
    if let Some(pagination) = pagination(model) {
        components.push(pagination);
    }
    json!({ "flags": FLAG, "components": components })
}

fn option(label: &str) -> Value {
    json!({ "label": label, "value": label })
}

fn pagination(model: &VoiceLeaderboardModel) -> Option<Value> {
    if model.pagination_disabled() || model.pages() <= 1 {
        return None;
    }
    let page = model.current_page();
    let pages = model.pages();
    Some(json!({
        "type": 1,
        "components": [
            {"type": 2, "custom_id": FIRST, "label": "⏮", "style": 1, "disabled": page == 1},
            {"type": 2, "custom_id": PREVIOUS, "label": "◀", "style": 1, "disabled": page == 1},
            {
                "type": 2,
                "custom_id": "current",
                "label": format!("{page}/{pages}"),
                "style": 2,
                "disabled": true
            },
            {"type": 2, "custom_id": NEXT, "label": "▶", "style": 1, "disabled": page == pages},
            {"type": 2, "custom_id": LAST, "label": "⏭", "style": 1, "disabled": page == pages}
        ]
    }))
}

pub fn translate(
    custom_id: &str,
    values: &[String],
    model: &VoiceLeaderboardModel,
) -> Result<VoiceLeaderboardMsg, String> {
    let message = match custom_id {
        TIME_RANGE => VoiceLeaderboardMsg::ChangeTimeRange(
            values
                .first()
                .and_then(|value| VoiceLeaderboardTimeRange::from_display_name(value))
                .ok_or_else(|| "invalid voice leaderboard time range".to_string())?,
        ),
        TOGGLE_MODE => VoiceLeaderboardMsg::ToggleMode,
        SELECT_USER => {
            VoiceLeaderboardMsg::SetTargetUser(values.first().and_then(|value| value.parse().ok()))
        }
        FIRST => VoiceLeaderboardMsg::Pagination(PaginationAction::First),
        PREVIOUS => VoiceLeaderboardMsg::Pagination(PaginationAction::Prev),
        NEXT => VoiceLeaderboardMsg::Pagination(PaginationAction::Next),
        LAST => VoiceLeaderboardMsg::Pagination(PaginationAction::Last),
        other => return Err(format!("unknown voice leaderboard action `{other}`")),
    };
    let _ = model;
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VoiceLeaderboardEntry;

    fn entry(user_id: u64, duration: i64) -> VoiceLeaderboardEntry {
        VoiceLeaderboardEntry {
            user_id,
            total_duration: duration,
        }
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

    fn normalized_components(model: &VoiceLeaderboardModel) -> Value {
        let mut value = components(model);
        normalize_timestamps(&mut value);
        value["components"].clone()
    }

    #[test]
    fn voice_leaderboard_render_server_snapshot() {
        let model =
            VoiceLeaderboardModel::from_entries(vec![entry(100, 3600), entry(200, 7200)], 100, 10);
        assert_eq!(
            normalized_components(&model),
            json!([
                {
                    "type": 17,
                    "components": [
                        { "type": 10, "content": "### Voice Leaderboard" },
                        { "type": 10, "content": "\nYou are ranked **#1** on this server with **1h** of voice activity." },
                        { "type": 10, "content": "\n-# Time Range: **This month** — <t:TS:f> to <t:TS:R>" },
                        { "type": 14, "divider": true },
                        {
                            "type": 12,
                            "items": [ { "media": { "url": "attachment://voice_leaderboard.jpg" } } ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": TOGGLE_MODE,
                                    "disabled": false,
                                    "label": "Show Voice Partners",
                                    "style": 1
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 3,
                            "custom_id": TIME_RANGE,
                            "options": [
                                { "label": "Past 24 hours", "value": "Past 24 hours" },
                                { "label": "Past 72 hours", "value": "Past 72 hours" },
                                { "label": "Past 7 days", "value": "Past 7 days" },
                                { "label": "Past 14 days", "value": "Past 14 days" },
                                { "label": "This month", "value": "This month" },
                                { "label": "This year", "value": "This year" },
                                { "label": "All time", "value": "All time" }
                            ],
                            "placeholder": "Select time range"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn voice_leaderboard_render_partner_snapshot() {
        let mut model =
            VoiceLeaderboardModel::from_entries(vec![entry(100, 3600), entry(200, 7200)], 100, 10);
        model.is_partner_mode = true;
        model.user_rank = None;
        model.user_duration = None;
        model.target_user_id = Some(200);
        model.target_user_name = Some("partner".to_string());

        assert_eq!(
            normalized_components(&model),
            json!([
                {
                    "type": 17,
                    "components": [
                        { "type": 10, "content": "### partner Voice Partners" },
                        { "type": 10, "content": "\nYou are not on the leaderboard for this time range." },
                        { "type": 10, "content": "\n-# Time Range: **This month** — <t:TS:f> to <t:TS:R>" },
                        { "type": 14, "divider": true },
                        {
                            "type": 12,
                            "items": [ { "media": { "url": "attachment://voice_leaderboard.jpg" } } ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": TOGGLE_MODE,
                                    "disabled": false,
                                    "label": "Show Server Leaderboard",
                                    "style": 1
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 3,
                            "custom_id": TIME_RANGE,
                            "options": [
                                { "label": "Past 24 hours", "value": "Past 24 hours" },
                                { "label": "Past 72 hours", "value": "Past 72 hours" },
                                { "label": "Past 7 days", "value": "Past 7 days" },
                                { "label": "Past 14 days", "value": "Past 14 days" },
                                { "label": "This month", "value": "This month" },
                                { "label": "This year", "value": "This year" },
                                { "label": "All time", "value": "All time" }
                            ],
                            "placeholder": "Select time range"
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 5,
                            "custom_id": SELECT_USER,
                            "default_values": [ { "id": 200, "type": "user" } ],
                            "placeholder": "Select an user to view their voice partners"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn toggle_mode_button_switches_to_partner_mode_and_requests_a_query() {
        let mut model = VoiceLeaderboardModel::from_entries(vec![], 100, 10);
        assert!(!model.is_partner_mode());
        let message = translate(TOGGLE_MODE, &[], &model).expect("translate toggle");
        assert_eq!(message, VoiceLeaderboardMsg::ToggleMode);

        let effects = crate::update::voice_leaderboard::update(message, &mut model);
        assert!(model.is_partner_mode());
        assert_eq!(
            effects,
            vec![
                crate::update::voice_leaderboard::VoiceLeaderboardEffect::QueryLeaderboard {
                    now: model.now,
                    time_range: VoiceLeaderboardTimeRange::ThisMonth,
                    is_partner_mode: true,
                    target_user_id: None,
                }
            ]
        );
        let rendered = components(&model);
        assert!(rendered.to_string().contains("Show Server Leaderboard"));
        assert!(rendered.to_string().contains("voice-leaderboard:user"));
    }
}
