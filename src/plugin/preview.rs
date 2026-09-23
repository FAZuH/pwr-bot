//! Host-side filling of the attachment slots a plugin envelope declares at
//! transport (ADR-0012).
//!
//! A plugin names an attachment by filename and never holds the bytes; the
//! host resolves the declaration through the renderer registry — one
//! [`AttachmentRenderer`] per filename — and attaches the rendered bytes.
//! This is consumer-side machinery: it belongs to the plugin transport
//! path, not to any feature shell.

use std::sync::Arc;

use async_trait::async_trait;
use log::debug;
use poise::serenity_prelude as serenity;
use serde_json::Value;
use serde_json::json;

use crate::bot::command::welcome::image_generator::WelcomeCardData;
use crate::bot::command::welcome::image_generator::WelcomeImageGenerator;
use crate::service::traits::FeedSubscriptionProvider;

/// Filename for the welcome preview image attachment.
pub const WELCOME_FILE: &str = "welcome_preview.png";

/// Renders one declared attachment filename into bytes for a guild. The
/// registry the [`PreviewResolver`] consults holds one implementation per
/// filename; a filename without a renderer is declared away.
#[async_trait]
pub trait AttachmentRenderer: Send + Sync {
    /// The filename this renderer fills, e.g. [`WELCOME_FILE`].
    fn filename(&self) -> &'static str;

    /// Renders the attachment bytes for the guild, or `None` to declare the
    /// slot away (feature disabled, render failure, …).
    async fn render(&self, guild_id: u64) -> Option<Vec<u8>>;
}

/// The welcome card renderer: generates the preview image from the guild's
/// current settings through the host's own image generator, keeping the
/// plugin free of rendering dependencies; the registry keeps the transport
/// renderer-agnostic.
pub struct WelcomeAttachmentRenderer {
    service: Arc<dyn FeedSubscriptionProvider>,
    generator: Arc<WelcomeImageGenerator>,
}

impl WelcomeAttachmentRenderer {
    /// Wraps the settings source the card renders from and the generator
    /// that renders it.
    pub fn new(
        service: Arc<dyn FeedSubscriptionProvider>,
        generator: Arc<WelcomeImageGenerator>,
    ) -> Self {
        Self { service, generator }
    }
}

#[async_trait]
impl AttachmentRenderer for WelcomeAttachmentRenderer {
    fn filename(&self) -> &'static str {
        WELCOME_FILE
    }

    async fn render(&self, guild_id: u64) -> Option<Vec<u8>> {
        let settings = self.service.get_server_settings(guild_id).await.ok()?;
        if !settings.welcome.enabled.unwrap_or(false) {
            return None;
        }
        // Preview uses placeholder data since we don't have a real member
        // context here
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
        self.generator.generate_card(data).await.ok()
    }
}

/// Fills the attachment slots a plugin envelope declares at transport
/// (ADR-0012): a slot the host can fill through its renderer registry
/// becomes an attached file; a slot the host cannot fill — no renderer, no
/// guild, a failed render — is declared away with an empty list, which
/// removes whatever the message carried.
pub struct PreviewResolver {
    renderers: Vec<Arc<dyn AttachmentRenderer>>,
}

impl PreviewResolver {
    /// Builds the resolver over the renderer registry, one renderer per
    /// filename it fills.
    pub fn new(renderers: Vec<Arc<dyn AttachmentRenderer>>) -> Self {
        Self { renderers }
    }

    /// Resolves `body`'s attachment declarations for the guild, returning
    /// the body to send and the files to send with it. A body that declares
    /// no slot is returned untouched with no files. A declaration the host
    /// cannot fill declares every slot away — the declared entries would
    /// reference files that were never uploaded.
    pub async fn resolve(
        &self,
        body: Value,
        guild_id: Option<u64>,
    ) -> (Value, Vec<serenity::CreateAttachment<'static>>) {
        let mut body = body;
        let Some(declared) = declared_filenames(&body) else {
            return (body, Vec::new());
        };
        let mut files = Vec::new();
        for filename in declared {
            let rendered = match guild_id {
                Some(guild_id) => match self.renderer(&filename) {
                    Some(renderer) => renderer.render(guild_id).await,
                    None => {
                        debug!("no renderer registered for declared attachment `{filename}`");
                        None
                    }
                },
                None => {
                    debug!("attachment `{filename}` declared outside a guild");
                    None
                }
            };
            match rendered {
                Some(bytes) => files.push(serenity::CreateAttachment::bytes(bytes, filename)),
                None => {
                    body["attachments"] = json!([]);
                    return (body, Vec::new());
                }
            }
        }
        (body, files)
    }

    fn renderer(&self, filename: &str) -> Option<&Arc<dyn AttachmentRenderer>> {
        self.renderers.iter().find(|r| r.filename() == filename)
    }
}

/// Collects the declared attachment filenames in send order; `None` when the
/// body declares no attachments at all.
fn declared_filenames(body: &Value) -> Option<Vec<String>> {
    let entries = body.get("attachments")?.as_array()?;
    if entries.is_empty() {
        return None;
    }
    Some(
        entries
            .iter()
            .filter_map(|entry| {
                entry
                    .get("filename")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::ServerSettings;

    /// A renderer over a fixed byte payload; `bytes` is served once per
    /// render call.
    struct FixedRenderer {
        filename: &'static str,
        bytes: Vec<u8>,
    }

    #[async_trait]
    impl AttachmentRenderer for FixedRenderer {
        fn filename(&self) -> &'static str {
            self.filename
        }

        async fn render(&self, _guild_id: u64) -> Option<Vec<u8>> {
            Some(self.bytes.clone())
        }
    }

    fn fixed_resolver(filename: &'static str) -> PreviewResolver {
        PreviewResolver::new(vec![Arc::new(FixedRenderer {
            filename,
            bytes: b"png".to_vec(),
        })])
    }

    /// A body declaring one slot by filename.
    fn slot_body(filename: &str) -> Value {
        json!({ "attachments": [{ "id": 0, "filename": filename }] })
    }

    #[tokio::test]
    async fn a_body_without_a_slot_is_untouched_with_no_files() {
        let resolver = fixed_resolver(WELCOME_FILE);
        let (body, files) = resolver
            .resolve(json!({ "components": [] }), Some(42))
            .await;
        assert_eq!(body, json!({ "components": [] }));
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn an_empty_declaration_is_untouched_with_no_files() {
        let resolver = fixed_resolver(WELCOME_FILE);
        let (body, files) = resolver
            .resolve(json!({ "attachments": [] }), Some(42))
            .await;
        assert_eq!(body["attachments"], json!([]));
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn a_registered_renderer_fills_the_slot_with_its_bytes() {
        let resolver = fixed_resolver(WELCOME_FILE);
        let (body, files) = resolver.resolve(slot_body(WELCOME_FILE), Some(42)).await;
        assert_eq!(
            body["attachments"],
            json!([{ "id": 0, "filename": WELCOME_FILE }])
        );
        assert_eq!(files.len(), 1);
    }

    #[tokio::test]
    async fn an_unregistered_filename_declares_the_slot_away() {
        let resolver = fixed_resolver(WELCOME_FILE);
        let (body, files) = resolver.resolve(slot_body("chart.png"), Some(42)).await;
        assert_eq!(body["attachments"], json!([]));
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn a_slot_outside_a_guild_declares_the_attachments_away() {
        let resolver = fixed_resolver(WELCOME_FILE);
        let (body, files) = resolver.resolve(slot_body(WELCOME_FILE), None).await;
        assert_eq!(body["attachments"], json!([]));
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn a_failed_render_declares_the_attachments_away() {
        struct FailingRenderer;
        #[async_trait]
        impl AttachmentRenderer for FailingRenderer {
            fn filename(&self) -> &'static str {
                WELCOME_FILE
            }

            async fn render(&self, _guild_id: u64) -> Option<Vec<u8>> {
                None
            }
        }
        let resolver = PreviewResolver::new(vec![Arc::new(FailingRenderer)]);
        let (body, files) = resolver.resolve(slot_body(WELCOME_FILE), Some(42)).await;
        assert_eq!(body["attachments"], json!([]));
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn the_welcome_renderer_gates_on_enabled_cards() {
        let mut mock = crate::service::traits::MockFeedSubscriptionProvider::new();
        mock.expect_get_server_settings()
            .with(mockall::predicate::eq(42))
            .times(1)
            .returning(|_| Ok(ServerSettings::default()));
        let renderer =
            WelcomeAttachmentRenderer::new(Arc::new(mock), Arc::new(WelcomeImageGenerator::new()));
        assert_eq!(renderer.render(42).await, None, "cards disabled by default");
    }

    #[test]
    fn declared_filenames_reads_the_declaration_in_order() {
        let body = json!({
            "attachments": [
                { "id": 0, "filename": "a.png" },
                { "id": 1 },
                { "id": 2, "filename": "b.png" },
            ]
        });
        assert_eq!(
            declared_filenames(&body),
            Some(vec!["a.png".to_string(), "b.png".to_string()])
        );
    }
}
