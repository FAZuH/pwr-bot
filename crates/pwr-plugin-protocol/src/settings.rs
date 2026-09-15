//! Server settings wire types: the whole [`ServerSettings`] snapshot that
//! crosses the host↔plugin seam through the `host.feed.get_settings` /
//! `host.feed.update_settings` ops (ADR-0010).
//!
//! These structs were the host crate's `entity.rs` types; they moved here so
//! the shared contract owns the payload both sides serialize. The host
//! re-exports them from `entity.rs`, so its imports stay stable, and the
//! diesel coupling stays behind in the host's `ServerSettingsEntity`.
//! `current_year`-style render-only values never appear here — a payload
//! field must be data the service itself stores.

use serde::Deserialize;
use serde::Serialize;

/// The whole per-guild settings snapshot: every feature section together.
/// The feed settings ops carry it in one piece, mirroring the service's
/// `get_server_settings`/`update_server_settings` pair and its
/// persist-the-whole-snapshot semantics.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct ServerSettings {
    #[serde(default)]
    pub feeds: FeedsSettings,
    #[serde(default)]
    pub voice: VoiceSettings,
    #[serde(default)]
    pub welcome: WelcomeSettings,
}

/// The feed subscription section: notification toggle, posting channel, and
/// the permission roles for (un)subscribing.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct FeedsSettings {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub subscribe_role_id: Option<String>,
    #[serde(default)]
    pub unsubscribe_role_id: Option<String>,
}

/// The voice tracking section.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct VoiceSettings {
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// The welcome message section.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct WelcomeSettings {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub primary_color: Option<String>,
    #[serde(default)]
    pub template_id: Option<String>,
    #[serde(default)]
    pub messages: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn sample() -> ServerSettings {
        ServerSettings {
            feeds: FeedsSettings {
                enabled: Some(true),
                channel_id: Some("123456789".into()),
                subscribe_role_id: Some("987654321".into()),
                unsubscribe_role_id: None,
            },
            voice: VoiceSettings {
                enabled: Some(false),
            },
            welcome: WelcomeSettings {
                enabled: None,
                channel_id: None,
                primary_color: Some("#5865F2".into()),
                template_id: None,
                messages: Some(vec!["welcome, {user}".into()]),
            },
        }
    }

    #[test]
    fn server_settings_serializes_to_declared_shape() {
        // `None` fields serialize as nulls: the snapshot is a fixed-shape
        // object, so a section absent from storage reads back as null
        // fields, not missing keys.
        assert_eq!(
            serde_json::to_value(sample()).unwrap(),
            json!({
                "feeds": {
                    "enabled": true,
                    "channel_id": "123456789",
                    "subscribe_role_id": "987654321",
                    "unsubscribe_role_id": null,
                },
                "voice": { "enabled": false },
                "welcome": {
                    "enabled": null,
                    "channel_id": null,
                    "primary_color": "#5865F2",
                    "template_id": null,
                    "messages": ["welcome, {user}"],
                },
            })
        );
    }

    #[test]
    fn server_settings_round_trips_losslessly() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert_eq!(
            serde_json::from_str::<ServerSettings>(&json).unwrap(),
            sample()
        );
    }

    #[test]
    fn missing_fields_default_instead_of_failing() {
        // A snapshot written before a section existed must still parse: every
        // field is `#[serde(default)]`, section-shaped or absent alike.
        assert_eq!(
            serde_json::from_value::<ServerSettings>(json!({ "feeds": {} })).unwrap(),
            ServerSettings::default()
        );
        assert_eq!(
            serde_json::from_str::<ServerSettings>("{}").unwrap(),
            ServerSettings::default()
        );
    }
}
