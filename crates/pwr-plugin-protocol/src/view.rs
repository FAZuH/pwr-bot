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
}
