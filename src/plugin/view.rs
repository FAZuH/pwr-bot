//! Host-side validation for plugin-provided Discord view payloads.

use pwr_ext::prelude::CreateMessageDe;
use pwr_plugin_protocol::WireError;
use serde_json::Value;

/// An error raised when a plugin view is not a valid Discord message payload.
#[derive(Debug, thiserror::Error)]
#[error("invalid plugin view message: {message}")]
pub struct ViewValidationError {
    /// The parser's detail, retained for the command error handler and logs.
    pub message: String,
}

impl ViewValidationError {
    /// Returns the protocol error representation for plugin-facing boundaries.
    pub fn wire_error(&self) -> WireError {
        WireError {
            kind: "InvalidView".into(),
            msg: self.to_string(),
        }
    }
}

impl From<ViewValidationError> for WireError {
    fn from(error: ViewValidationError) -> Self {
        error.wire_error()
    }
}

/// Validates a plugin view without changing the value that the host sends.
///
/// Parsing is deliberately performed on a clone. The parsed builder mirror is
/// discarded so Discord receives the plugin's original JSON representation.
pub fn validate_view_data(data: &Value) -> Result<(), ViewValidationError> {
    serde_json::from_value::<CreateMessageDe>(data.clone())
        .map(|_| ())
        .map_err(|error| ViewValidationError {
            message: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_message() -> Value {
        serde_json::json!({
            "content": "Choose an option ✨",
            "nonce": "view-42",
            "tts": false,
            "embeds": [],
            "allowed_mentions": {"parse": []},
            "message_reference": null,
            "components": [{
                "type": 1,
                "components": [{
                    "type": 2,
                    "style": 1,
                    "custom_id": "choose:42",
                    "label": "Choose",
                    "disabled": false
                }]
            }],
            "sticker_ids": [],
            "flags": null,
            "attachments": [],
            "enforce_nonce": false,
            "poll": null
        })
    }

    #[test]
    fn accepts_a_complete_message() {
        assert!(validate_view_data(&complete_message()).is_ok());
    }

    #[test]
    fn rejects_a_malformed_component() {
        let data = serde_json::json!({
            "content": "Choose",
            "tts": false,
            "components": [{"type": 1, "components": [{"type": 2}]}],
            "enforce_nonce": false
        });

        let error = validate_view_data(&data).unwrap_err();

        assert!(error.to_string().contains("invalid plugin view message"));
    }

    #[test]
    fn rejects_a_malformed_message() {
        let data = serde_json::json!({"content": "missing required message fields"});

        assert!(validate_view_data(&data).is_err());
    }

    #[test]
    fn rejects_a_non_message_value() {
        let data = serde_json::json!([{"type": 2, "style": 1, "custom_id": "not-a-message"}]);

        assert!(validate_view_data(&data).is_err());
    }

    #[test]
    fn validation_does_not_change_the_original_json() {
        let data = complete_message();
        let original = data.clone();

        validate_view_data(&data).expect("fixture must be valid");

        assert_eq!(data, original);
    }
}
