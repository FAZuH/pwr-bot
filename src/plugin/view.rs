//! Host-side validation for plugin-provided Discord view payloads.

use pwr_ext::prelude::CreateMessageDe;
use pwr_plugin_protocol::WireError;
use pwr_poise_components::IS_COMPONENTS_V2;
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
    serde_json::from_value::<CreateMessageDe>(data.clone()).map_err(|error| {
        ViewValidationError {
            message: error.to_string(),
        }
    })?;
    reject_content_beside_v2(data)
}

/// Rejects a non-empty top-level `content` on a Components V2 payload: the
/// schema parse accepts the combination but Discord rejects it with error
/// 50035, so the gate refuses it before any send. Any `content` that Discord
/// would treat as set is rejected regardless of its JSON type — only `null`
/// (the clear value) and the empty string pass. Runs inside
/// [`validate_view_data`]. The `host.edit_message` arm runs the strict
/// [`reject_content_on_edit`] instead.
pub fn reject_content_beside_v2(data: &Value) -> Result<(), ViewValidationError> {
    let v2 = data
        .get("flags")
        .and_then(Value::as_u64)
        .is_some_and(|flags| flags & u64::from(IS_COMPONENTS_V2) != 0);
    if v2 && content_is_set(data) {
        return Err(ViewValidationError {
            message: "cannot use legacy content with components V2".into(),
        });
    }
    Ok(())
}

/// Rejects any top-level `content` Discord would treat as set on a
/// `host.edit_message` payload, regardless of `flags`. Discord cannot unset
/// `IS_COMPONENTS_V2` when editing, so content beside absent flags still
/// fails with error 50035 against an already-V2 message — the edit arm must
/// therefore reject every set `content`, not only one beside the V2 flag.
pub fn reject_content_on_edit(data: &Value) -> Result<(), ViewValidationError> {
    if content_is_set(data) {
        return Err(ViewValidationError {
            message: "cannot use legacy content when editing a components V2 message".into(),
        });
    }
    Ok(())
}

/// True when `data` carries a top-level `content` Discord would treat as
/// set: anything present other than `null` (the clear value) or the empty
/// string. A non-string `content` counts as set so a type-confused value
/// cannot slip past the gate.
fn content_is_set(data: &Value) -> bool {
    match data.get("content") {
        None | Some(Value::Null) => false,
        Some(Value::String(content)) => !content.is_empty(),
        Some(_) => true,
    }
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
    fn rejects_a_non_empty_content_beside_the_v2_flag() {
        let data = serde_json::json!({
            "content": "hello",
            "tts": false,
            "enforce_nonce": false,
            "components": [{"type": 10, "content": "hi"}],
            "flags": 32768,
        });

        let error = validate_view_data(&data).unwrap_err();

        assert!(error.to_string().contains("legacy content"));
        assert!(error.to_string().contains("components V2"));
    }

    #[test]
    fn rejects_a_non_string_content_beside_the_v2_flag() {
        // A type-confused `content` is still content beside V2: Discord
        // rejects the payload, so only `null` (the clear value) and the
        // empty string may pass the gate.
        let type_confused_contents = [
            serde_json::json!(42),
            serde_json::json!(true),
            serde_json::json!({}),
        ];
        for content in type_confused_contents {
            let data = serde_json::json!({
                "content": content,
                "components": [{"type": 10, "content": "hi"}],
                "flags": 32768,
            });

            assert!(
                reject_content_beside_v2(&data).is_err(),
                "non-string content {content} beside V2 must be rejected"
            );
        }
    }

    #[test]
    fn accept_an_empty_content_on_a_v2_payload() {
        let data = serde_json::json!({
            "content": "",
            "tts": false,
            "enforce_nonce": false,
            "components": [{"type": 10, "content": "hi"}],
            "flags": 32768,
        });

        assert!(validate_view_data(&data).is_ok());
    }

    #[test]
    fn reject_content_beside_v2_checks_only_top_level_fields() {
        // A text-display component's own `content` is the V2 way to carry
        // text: the check must not reject it.
        let data = serde_json::json!({
            "tts": false,
            "enforce_nonce": false,
            "components": [
                {"type": 17, "components": [{"type": 10, "content": "hello"}]}
            ],
            "flags": 32768,
        });

        assert!(reject_content_beside_v2(&data).is_ok());
        assert!(validate_view_data(&data).is_ok());
    }

    #[test]
    fn accepts_a_null_content_on_a_v2_payload() {
        // `null` is Discord's clear value, so it counts as absent.
        let data = serde_json::json!({
            "content": null,
            "components": [{"type": 10, "content": "hi"}],
            "flags": 32768,
        });

        assert!(reject_content_beside_v2(&data).is_ok());
    }

    #[test]
    fn reject_content_on_edit_rejects_content_without_any_flags() {
        // Discord cannot unset IS_COMPONENTS_V2 on edit, so content beside
        // absent flags still fails with 50035 against an already-V2 message.
        let data = serde_json::json!({"content": "edited prose"});

        let error = reject_content_on_edit(&data).unwrap_err();

        assert!(error.to_string().contains("legacy content"));
    }

    #[test]
    fn reject_content_on_edit_rejects_content_beside_the_v2_flag() {
        let data = serde_json::json!({"content": "edited prose", "flags": 32768});

        assert!(reject_content_on_edit(&data).is_err());
    }

    #[test]
    fn reject_content_on_edit_rejects_a_non_string_content() {
        let data = serde_json::json!({"content": 42});

        assert!(reject_content_on_edit(&data).is_err());
    }

    #[test]
    fn reject_content_on_edit_accepts_the_clearing_forms() {
        assert!(reject_content_on_edit(&serde_json::json!({"content": ""})).is_ok());
        assert!(reject_content_on_edit(&serde_json::json!({"content": null})).is_ok());
        assert!(reject_content_on_edit(&serde_json::json!({"components": []})).is_ok());
    }

    #[test]
    fn validation_does_not_change_the_original_json() {
        let data = complete_message();
        let original = data.clone();

        validate_view_data(&data).expect("fixture must be valid");

        assert_eq!(data, original);
    }
}
