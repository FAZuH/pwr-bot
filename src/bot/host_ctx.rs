use std::sync::Arc;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use poise::serenity_prelude::*;

use crate::bot::Data;
use crate::bot::command::Context as BotContext;
use crate::bot::command::Error;

/// Owned message payload for sending across trait boundaries.
///
/// Wraps raw JSON that the host forwards to Discord's API.
/// For interaction responses the host wraps `data` in `{"type": 4, "data": …}`;
/// for edits / channel messages / DMs the value is sent as-is.
pub struct MessagePayload(pub serde_json::Value);

/// Abstract interface for Discord interaction operations.
///
/// Built-in commands use [`PoiseHostCtx`] (wrapping poise's `Context`).
/// Plugin commands use `FfiHostCtx` (calling back into the host via FFI).
#[async_trait]
pub trait HostCtx: Send + Sync {
    /// Returns the guild ID for this interaction, if any.
    fn guild_id(&self) -> Option<u64>;
    /// Returns the Discord user ID of the interaction author.
    fn author_id(&self) -> u64;
    /// Returns the Discord channel ID where the interaction occurred.
    fn channel_id(&self) -> u64;
    /// Returns a clone of the shared application [`Data`].
    fn data(&self) -> Arc<Data>;

    /// Defers the interaction, showing a loading state to the user.
    async fn defer(&self) -> Result<(), Error>;
    /// Sends a (possibly ephemeral) message in response to the interaction.
    async fn send_message(&self, payload: &MessagePayload) -> Result<MessageId, Error>;
    /// Edits a previously sent message.
    async fn edit_message(
        &self,
        message_id: MessageId,
        payload: &MessagePayload,
    ) -> Result<(), Error>;
    /// Acknowledges a component interaction without sending a message.
    async fn acknowledge(&self, interaction: &ComponentInteraction) -> Result<(), Error>;
}

/// Interaction not yet responded to.
const PENDING: u8 = 0b00;
/// A full message response was sent as the initial interaction response.
const RESPONDED: u8 = 0b01;
/// A defer (type 5) was sent; the actual response must edit the original.
const DEFERRED: u8 = 0b11;

/// HostCtx backed by a live poise `Context`.
///
/// Extracts all owned state at construction so the type is lifetime-free.
pub struct PoiseHostCtx {
    guild_id: Option<u64>,
    author_id: u64,
    channel_id: u64,
    data: Arc<Data>,
    http: Arc<Http>,
    interaction: Option<CommandInteraction>,
    /// Interaction response state: PENDING (0b00), RESPONDED (0b01), DEFERRED (0b11).
    state: AtomicU8,
}

impl PoiseHostCtx {
    /// Creates a new `PoiseHostCtx` from a poise command context.
    ///
    /// Extracts all owned state (guild ID, author ID, channel ID, HTTP client)
    /// at construction so the type is lifetime-free and can be sent across tasks.
    pub fn new(ctx: BotContext<'_>) -> Arc<Self> {
        let interaction = match ctx {
            BotContext::Application(app_ctx) => Some(app_ctx.interaction.clone()),
            BotContext::Prefix(_) => None,
        };

        let http = ctx.serenity_context().http.clone();

        Arc::new(Self {
            guild_id: ctx.guild_id().map(|g| g.get()),
            author_id: ctx.author().id.get(),
            channel_id: ctx.channel_id().into(),
            data: ctx.data(),
            http,
            interaction,
            state: AtomicU8::new(PENDING),
        })
    }

    /// Creates a headless system context (no interaction — for events/tasks).
    pub fn new_system(data: Arc<Data>, http: Arc<Http>) -> Arc<Self> {
        Arc::new(Self {
            guild_id: None,
            author_id: 0,
            channel_id: 0,
            data,
            http,
            interaction: None,
            state: AtomicU8::new(PENDING),
        })
    }

    /// Creates a system context with channel/guild/author metadata.
    ///
    /// Used for component interaction dispatch where the plugin needs to edit
    /// the message, which requires knowing the channel ID.
    pub fn new_system_with_channel(
        data: Arc<Data>,
        http: Arc<Http>,
        channel_id: u64,
        guild_id: Option<u64>,
        author_id: u64,
    ) -> Arc<Self> {
        Arc::new(Self {
            guild_id,
            author_id,
            channel_id,
            data,
            http,
            interaction: None,
            state: AtomicU8::new(PENDING),
        })
    }

    /// Marks as responded (full message, not defer).
    ///
    /// Used by [`gui_test`](crate::bot::command::gui_test) to prevent plugin
    /// dispatch from trying to defer an already-responded interaction.
    pub fn mark_responded(&self) {
        self.state.store(RESPONDED, Ordering::SeqCst);
    }

    /// Returns `true` if the interaction has already received any response
    /// (either a full message or a defer).
    pub fn was_responded(&self) -> bool {
        self.state.load(Ordering::SeqCst) != PENDING
    }

    /// Returns a reference to the Discord HTTP client.
    pub fn http(&self) -> &Arc<Http> {
        &self.http
    }

    /// Returns a clone of the shared application [`Data`].
    pub fn data(&self) -> Arc<Data> {
        self.data.clone()
    }

    fn cmd_interaction(&self) -> Option<&CommandInteraction> {
        self.interaction.as_ref()
    }
}

#[async_trait]
impl HostCtx for PoiseHostCtx {
    fn guild_id(&self) -> Option<u64> {
        self.guild_id
    }

    fn author_id(&self) -> u64 {
        self.author_id
    }

    fn channel_id(&self) -> u64 {
        self.channel_id
    }

    fn data(&self) -> Arc<Data> {
        self.data.clone()
    }

    async fn defer(&self) -> Result<(), Error> {
        // No-op. Plugins run fast enough (<1s from logs) that we skip the
        // Discord defer entirely. send_message will send the response
        // directly as the initial interaction response (type 4).
        Ok(())
    }

    async fn send_message(&self, payload: &MessagePayload) -> Result<MessageId, Error> {
        if let Some(cmd) = self.cmd_interaction() {
            let prev = self.state.load(Ordering::SeqCst);
            match prev {
                PENDING => {
                    tracing::debug!("send_message: state=PENDING, attempting initial response");
                    if self
                        .state
                        .compare_exchange(PENDING, RESPONDED, Ordering::SeqCst, Ordering::SeqCst)
                        .is_err()
                    {
                        tracing::debug!("send_message: CAS failed, falling through to followup");
                    } else {
                        let envelope = serde_json::json!({
                            "type": 4,
                            "data": payload.0,
                        });
                        tracing::debug!(
                            interaction.id = %cmd.id,
                            "send_message: sending initial response via HTTP",
                        );
                        self.http
                            .create_interaction_response(cmd.id, &cmd.token, &envelope, vec![])
                            .await?;
                        let msg = self
                            .http
                            .get_original_interaction_response(&cmd.token)
                            .await?;
                        return Ok(msg.id);
                    }
                }
                DEFERRED => {
                    self.http
                        .edit_original_interaction_response(&cmd.token, &payload.0, vec![])
                        .await?;
                    let msg = self
                        .http
                        .get_original_interaction_response(&cmd.token)
                        .await?;
                    return Ok(msg.id);
                }
                _ => {}
            }

            // Already responded — send as a followup.
            let msg = self
                .http
                .create_followup_message(&cmd.token, &payload.0, vec![])
                .await?;
            Ok(msg.id)
        } else {
            // Prefix context — send a channel message
            Ok(self
                .http
                .send_message(GenericChannelId::new(self.channel_id), vec![], &payload.0)
                .await?
                .id)
        }
    }

    async fn edit_message(
        &self,
        message_id: MessageId,
        payload: &MessagePayload,
    ) -> Result<(), Error> {
        if let Some(cmd) = self.cmd_interaction() {
            self.http
                .edit_original_interaction_response(&cmd.token, &payload.0, vec![])
                .await?;
        } else {
            self.http
                .edit_message(
                    GenericChannelId::new(self.channel_id),
                    message_id,
                    &payload.0,
                    vec![],
                )
                .await?;
        }
        Ok(())
    }

    async fn acknowledge(&self, interaction: &ComponentInteraction) -> Result<(), Error> {
        interaction
            .create_response(&self.http, CreateInteractionResponse::Acknowledge)
            .await?;
        Ok(())
    }
}
