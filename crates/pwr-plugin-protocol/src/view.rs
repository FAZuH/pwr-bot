//! ViewSpec: the raw Discord message spec a plugin view renders as.
//!
//! The host renders views from this spec verbatim (raw Discord message JSON,
//! no serenity/poise types involved) and stores the opaque `view` value,
//! returning it to the plugin when an interaction arrives.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// The raw Discord message spec for a plugin view: `data` is the message
/// content, `view` is opaque plugin state the host stores and hands back on
/// interactions, and `ephemeral` controls message visibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewSpec {
    /// Message payload (content, embeds, components — raw Discord JSON).
    pub data: Value,
    /// `true` if the message is visible only to the invoking user.
    pub ephemeral: bool,
    /// Opaque plugin state/params, stored by the host and returned verbatim on
    /// interactions with the view.
    pub view: Value,
}

/// A view request's payload, interpreted from the two wire shapes a plugin
/// may return:
///
/// - the envelope `{"data": …, "ephemeral": …, "view": …}` — the message
///   rides in `data`, visibility and opaque state beside it. Discord
///   messages have no top-level `data` field, so the key check is
///   unambiguous;
/// - the legacy raw shape: the Discord message JSON itself.
///
/// [`view_payload`] classifies a payload into one of these; callers then
/// project the fields they care about (the host reads all three, the preview
/// reads only `data`).
#[derive(Debug, Clone, PartialEq)]
pub enum ViewPayload {
    /// The full envelope shape: the message plus optional visibility and
    /// opaque state. `view` is `None` when the envelope carries no `view`
    /// field, so the caller can fall back to its own stored state.
    Envelope {
        /// The message payload.
        data: Value,
        /// `true` when the envelope carries `"ephemeral": true`; a missing or
        /// non-boolean `ephemeral` is `false`.
        ephemeral: bool,
        /// The opaque `view` value, present when the envelope carries one.
        view: Option<Value>,
    },
    /// The legacy raw shape: the payload is the Discord message JSON itself.
    Raw {
        /// The message payload, verbatim.
        data: Value,
    },
}

/// Classifies a view-request payload into its wire shape. An object with a
/// `"data"` key is an [`ViewPayload::Envelope`]; everything else is
/// [`ViewPayload::Raw`]. Infallible.
pub fn view_payload(data: &Value) -> ViewPayload {
    match data {
        Value::Object(map) if map.contains_key("data") => ViewPayload::Envelope {
            data: map.get("data").cloned().unwrap_or_default(),
            ephemeral: map
                .get("ephemeral")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            view: map.get("view").cloned(),
        },
        _ => ViewPayload::Raw { data: data.clone() },
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn view_spec_round_trips_losslessly() {
        let spec = ViewSpec {
            data: json!({"content": "hello", "components": []}),
            ephemeral: true,
            view: json!({"guild_id": "123", "page": 2}),
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert_eq!(serde_json::from_str::<ViewSpec>(&json).unwrap(), spec);
    }

    #[test]
    fn view_spec_serializes_to_declared_shape() {
        let spec = ViewSpec {
            data: json!({"content": "hello"}),
            ephemeral: false,
            view: json!({"guild_id": "123"}),
        };
        assert_eq!(
            serde_json::to_string(&spec).unwrap(),
            r#"{"data":{"content":"hello"},"ephemeral":false,"view":{"guild_id":"123"}}"#
        );
    }

    #[test]
    fn null_view_and_data_round_trip() {
        let spec = ViewSpec {
            data: Value::Null,
            ephemeral: false,
            view: Value::Null,
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert_eq!(serde_json::from_str::<ViewSpec>(&json).unwrap(), spec);
    }

    // ── payload classification ───────────────────────────────────────────────

    #[test]
    fn envelope_with_all_keys_classifies_as_envelope() {
        let payload = json!({
            "data": {"content": "hello"},
            "ephemeral": true,
            "view": {"page": 3},
        });
        assert_eq!(
            view_payload(&payload),
            ViewPayload::Envelope {
                data: json!({"content": "hello"}),
                ephemeral: true,
                view: Some(json!({"page": 3})),
            }
        );
    }

    #[test]
    fn envelope_without_ephemeral_defaults_to_false() {
        let payload = json!({
            "data": {"content": "hello"},
            "view": {"page": 3},
        });
        assert_eq!(
            view_payload(&payload),
            ViewPayload::Envelope {
                data: json!({"content": "hello"}),
                ephemeral: false,
                view: Some(json!({"page": 3})),
            }
        );
    }

    #[test]
    fn envelope_without_view_exposes_none() {
        let payload = json!({
            "data": {"content": "hello"},
            "ephemeral": false,
        });
        assert_eq!(
            view_payload(&payload),
            ViewPayload::Envelope {
                data: json!({"content": "hello"}),
                ephemeral: false,
                view: None,
            }
        );
    }

    #[test]
    fn envelope_with_non_boolean_ephemeral_falls_back_to_false() {
        // A present-but-non-boolean `ephemeral` is treated as absent (the
        // host's `as_bool` semantics): the message is not hidden.
        let payload = json!({
            "data": {"content": "hello"},
            "ephemeral": "yes",
            "view": null,
        });
        assert_eq!(
            view_payload(&payload),
            ViewPayload::Envelope {
                data: json!({"content": "hello"}),
                ephemeral: false,
                view: Some(Value::Null),
            }
        );
    }

    #[test]
    fn raw_object_without_data_key_classifies_as_raw() {
        let payload = json!({"content": "hi", "components": []});
        assert_eq!(
            view_payload(&payload),
            ViewPayload::Raw {
                data: payload.clone()
            }
        );
    }

    #[test]
    fn raw_non_object_classifies_as_raw() {
        assert_eq!(
            view_payload(&json!("just a string")),
            ViewPayload::Raw {
                data: json!("just a string")
            }
        );
        assert_eq!(
            view_payload(&json!([1, 2, 3])),
            ViewPayload::Raw {
                data: json!([1, 2, 3])
            }
        );
    }
}
