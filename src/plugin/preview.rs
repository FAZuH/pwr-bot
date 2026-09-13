//! Host-side filling of the attachment slots a plugin envelope declares at
//! transport (ADR-0012).
//!
//! The welcome settings plugin names the preview by filename and never holds
//! image bytes, so the host renders the card from the guild's current
//! settings and attaches it here. This is consumer-side machinery: it belongs
//! to the plugin transport path, not to any feature shell.

use std::sync::Arc;

use log::debug;
use poise::serenity_prelude as serenity;
use serde_json::Value;
use serde_json::json;

use crate::bot::command::welcome::image_generator::WelcomeCardData;
use crate::bot::command::welcome::image_generator::WelcomeImageGenerator;
use crate::entity::ServerSettings;
use crate::service::traits::FeedSubscriptionProvider;

/// Filename for the welcome preview image attachment.
pub const WELCOME_FILE: &str = "welcome_preview.png";

/// Generates a welcome card preview given settings and generator.
async fn generate_preview_from(
    settings: &ServerSettings,
    generator: &WelcomeImageGenerator,
) -> Option<Vec<u8>> {
    if !settings.welcome.enabled.unwrap_or(false) {
        return None;
    }
    // Preview uses placeholder data since we don't have a real member context here
    let data = WelcomeCardData {
        template_id: settings
            .welcome
            .template_id
            .clone()
            .unwrap_or_else(|| "1".to_string()),
        username: "PreviewUser".to_string(),
        user_tag: "@previewuser".to_string(),
        avatar_url: String::new(),
        avatar_b64: None,
        server_name: "Your Server".to_string(),
        member_count: "100".to_string(),
        member_number: "#100".to_string(),
        primary_color: settings
            .welcome
            .primary_color
            .clone()
            .unwrap_or_else(|| "#5865F2".to_string()),
        welcome_message: settings
            .welcome
            .messages
            .as_ref()
            .and_then(|m| m.first())
            .cloned()
            .unwrap_or_else(|| "Welcome to the server!".to_string()),
    };
    generator.generate_card(data).await.ok()
}

/// Fills the attachment slots a plugin envelope declares at transport
/// (ADR-0012): the welcome settings plugin names the preview by filename and
/// never holds image bytes, so the host renders the card from the guild's
/// current settings and attaches it here. A slot the host cannot fill — no
/// guild, no settings, a failed render — is declared away with an empty list,
/// which removes whatever the message carried.
pub struct PreviewResolver {
    service: Arc<dyn FeedSubscriptionProvider>,
    generator: Arc<WelcomeImageGenerator>,
}

impl PreviewResolver {
    /// Wraps the settings source the card renders from and the generator
    /// that renders it.
    pub fn new(
        service: Arc<dyn FeedSubscriptionProvider>,
        generator: Arc<WelcomeImageGenerator>,
    ) -> Self {
        Self { service, generator }
    }

    /// Resolves `body`'s attachment declaration for the guild whose settings
    /// the card renders from, returning the body to send and the files to
    /// send with it. A body that declares no preview slot is returned
    /// untouched with no files.
    pub async fn resolve(
        &self,
        mut body: Value,
        guild_id: Option<u64>,
    ) -> (Value, Vec<serenity::CreateAttachment<'static>>) {
        if !declares_preview(&body) {
            return (body, Vec::new());
        }
        let settings = match guild_id {
            Some(guild_id) => self.service.get_server_settings(guild_id).await.ok(),
            None => {
                debug!("welcome preview declared outside a guild");
                None
            }
        };
        let bytes = match settings.as_ref() {
            Some(settings) => generate_preview_from(settings, &self.generator).await,
            None => None,
        };
        match bytes {
            Some(bytes) => (
                body,
                vec![serenity::CreateAttachment::bytes(bytes, WELCOME_FILE)],
            ),
            None => {
                body["attachments"] = json!([]);
                (body, Vec::new())
            }
        }
    }
}

/// Whether an edit body declares the welcome preview slot by filename.
fn declares_preview(body: &Value) -> bool {
    body.get("attachments")
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.get("filename").and_then(Value::as_str) == Some(WELCOME_FILE))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::traits::MockFeedSubscriptionProvider;

    /// A body declaring the welcome preview slot, as the plugin envelope
    /// does while welcome cards are enabled.
    fn slot_body() -> Value {
        json!({ "attachments": [{ "id": 0, "filename": WELCOME_FILE }] })
    }

    /// A resolver over a mock service: `expectation` serves the settings
    /// load the slot resolution issues for a guild.
    fn resolver(expectation: impl FnOnce(&mut MockFeedSubscriptionProvider)) -> PreviewResolver {
        let mut mock = MockFeedSubscriptionProvider::new();
        expectation(&mut mock);
        PreviewResolver::new(Arc::new(mock), Arc::new(WelcomeImageGenerator::new()))
    }

    #[tokio::test]
    async fn a_body_without_a_slot_is_untouched_with_no_files() {
        let resolver = resolver(|mock| {
            mock.expect_get_server_settings().times(0);
        });
        let (body, files) = resolver
            .resolve(json!({ "components": [] }), Some(42))
            .await;
        assert_eq!(body, json!({ "components": [] }));
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn a_slot_outside_a_guild_declares_the_attachments_away() {
        let resolver = resolver(|mock| {
            mock.expect_get_server_settings().times(0);
        });
        let (body, files) = resolver.resolve(slot_body(), None).await;
        assert_eq!(body["attachments"], json!([]));
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn a_failed_settings_load_declares_the_attachments_away() {
        let resolver = resolver(|mock| {
            mock.expect_get_server_settings()
                .with(mockall::predicate::eq(42))
                .times(1)
                .returning(|_| {
                    Err(crate::service::error::ServiceError::UnexpectedResult {
                        message: "guild gone".into(),
                    })
                });
        });
        let (body, files) = resolver.resolve(slot_body(), Some(42)).await;
        assert_eq!(body["attachments"], json!([]));
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn a_guild_with_disabled_cards_declares_the_attachments_away() {
        let resolver = resolver(|mock| {
            mock.expect_get_server_settings()
                .with(mockall::predicate::eq(42))
                .times(1)
                .returning(|_| Ok(ServerSettings::default()));
        });
        let (body, files) = resolver.resolve(slot_body(), Some(42)).await;
        assert_eq!(body["attachments"], json!([]));
        assert!(files.is_empty());
    }
}
