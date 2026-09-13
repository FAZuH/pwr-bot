//! Host-side validation for plugin-provided Discord view payloads, and the
//! transport preparation that turns a validated payload into an edit body.

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

/// The message fields only the create endpoint accepts: `PATCH
/// /channels/{channel.id}/messages/{message.id}` takes `content`, `embeds`,
/// `flags`, `allowed_mentions`, `components` and `attachments`, and nothing
/// else of what a create body may carry.
const CREATE_ONLY_FIELDS: [&str; 6] = [
    "sticker_ids",
    "tts",
    "nonce",
    "enforce_nonce",
    "poll",
    "message_reference",
];

/// Prepares a validated view payload for the edit transport: returns `data`
/// without the create-only fields.
///
/// A view payload is built for sending, so it carries the create fields the
/// `view!` macro emits — `sticker_ids` above all, which Discord rejects on
/// edit with error 50080 ("Cannot edit stickers within a message") even when
/// the array is empty. Every edit transport site runs this helper on the body
/// before handing it to Discord. Fields the edit endpoint accepts —
/// `components`, `flags`, `embeds`, `attachments`, `content`,
/// `allowed_mentions` — pass through untouched, and so does anything the
/// helper does not know.
///
/// This is transport preparation, not validation: the gate
/// ([`validate_view_data`]) refuses invalid payloads and never rewrites them.
pub fn edit_body_for_transport(data: &Value) -> Value {
    let mut body = data.clone();
    let Some(fields) = body.as_object_mut() else {
        return body;
    };
    for field in CREATE_ONLY_FIELDS {
        fields.remove(field);
    }
    body
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

    // ── edit_body_for_transport ─────────────────────────────────────────────

    fn create_envelope() -> Value {
        // The shape the `view!` macro emits: a full create body with the
        // Components V2 flag set. `message_reference`, `nonce`, and `poll`
        // are the other create-only fields a payload could carry.
        serde_json::json!({
            "content": "",
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
            "embeds": [],
            "attachments": [],
            "allowed_mentions": {"parse": []},
            "sticker_ids": [],
            "tts": false,
            "nonce": "view-42",
            "enforce_nonce": false,
            "poll": null,
            "message_reference": null,
            "flags": 32768
        })
    }

    #[test]
    fn edit_body_for_transport_keeps_edit_fields_and_drops_create_only_fields() {
        let body = edit_body_for_transport(&create_envelope());

        // Everything the edit endpoint accepts survives verbatim.
        assert!(body.get("components").is_some());
        assert_eq!(body["flags"], 32768);
        assert_eq!(body["embeds"], serde_json::json!([]));
        assert_eq!(body["attachments"], serde_json::json!([]));
        assert_eq!(body["allowed_mentions"], serde_json::json!({"parse": []}));
        assert_eq!(body["content"], serde_json::json!(""));
        // Everything the edit endpoint refuses or ignores is gone.
        assert!(body.get("sticker_ids").is_none());
        assert!(body.get("tts").is_none());
        assert!(body.get("nonce").is_none());
        assert!(body.get("enforce_nonce").is_none());
        assert!(body.get("poll").is_none());
        assert!(body.get("message_reference").is_none());
    }

    #[test]
    fn edit_body_for_transport_leaves_a_partial_edit_body_unchanged() {
        // Edits are partial: a body without any create-only field comes back
        // exactly as it went in.
        let data = serde_json::json!({"components": [{"type": 10, "content": "hi"}]});
        let original = data.clone();

        assert_eq!(edit_body_for_transport(&data), original);
    }

    #[test]
    fn edit_body_for_transport_passes_unknown_fields_through() {
        // The helper drops only the fields it knows are create-only; a
        // field Discord adds later is not silently rewritten.
        let data = serde_json::json!({"some_future_field": 1, "components": []});

        let body = edit_body_for_transport(&data);

        assert_eq!(body["some_future_field"], 1);
    }

    #[test]
    fn edit_body_for_transport_does_not_change_the_original_json() {
        let data = create_envelope();
        let original = data.clone();

        edit_body_for_transport(&data);

        assert_eq!(data, original);
    }

    #[test]
    fn edit_body_for_transport_leaves_a_non_object_payload_alone() {
        assert_eq!(
            edit_body_for_transport(&serde_json::json!([])),
            serde_json::json!([])
        );
        assert_eq!(
            edit_body_for_transport(&serde_json::json!("prose")),
            serde_json::json!("prose")
        );
    }

    #[test]
    fn a_create_envelope_survives_the_gate_then_sanitizes_to_a_legal_edit_body() {
        // The composition every edit transport performs: the gate validates
        // the payload as sent, then the helper strips what edit cannot
        // carry. The result is a partial edit body — the transport sends it
        // verbatim, so it never needs to parse as a full create envelope.
        let data = create_envelope();
        validate_view_data(&data).expect("create envelope must pass the gate");

        let body = edit_body_for_transport(&data);

        for field in [
            "sticker_ids",
            "tts",
            "nonce",
            "enforce_nonce",
            "poll",
            "message_reference",
        ] {
            assert!(body.get(field).is_none(), "`{field}` must not ride an edit");
        }
        assert_eq!(body["flags"], 32768);
        assert_eq!(
            body["components"][0]["components"][0]["custom_id"],
            serde_json::json!("choose:42")
        );
    }
}
