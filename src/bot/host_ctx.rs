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
/// Constructed by handlers and consumed by [`HostCtx::send_message`] or
/// [`HostCtx::edit_message`].
pub struct MessagePayload {
    pub content: Option<String>,
    pub embed: Option<CreateEmbed<'static>>,
    pub ephemeral: bool,
}

impl MessagePayload {
    /// Creates a plain text message payload (non-ephemeral, no embed).
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: Some(content.into()),
            embed: None,
            ephemeral: false,
        }
    }
}

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

    fn build_response_message<'a>(
        &self,
        payload: &'a MessagePayload,
    ) -> CreateInteractionResponseMessage<'a> {
        let mut msg = CreateInteractionResponseMessage::new();
        if let Some(ref content) = payload.content {
            msg = msg.content(content);
        }
        if let Some(ref embed) = payload.embed {
            msg = msg.embed(embed.clone());
        }
        if payload.ephemeral {
            msg = msg.ephemeral(true);
        }
        msg
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
                        let builder = self.build_response_message(payload);
                        tracing::debug!(
                            interaction.id = %cmd.id,
                            interaction.token = %cmd.token,
                            "send_message: calling cmd.create_response",
                        );
                        match cmd.create_response(
                            &self.http,
                            CreateInteractionResponse::Message(builder),
                        )
                        .await
                        {
                            Ok(()) => tracing::debug!("send_message: create_response succeeded"),
                            Err(e) => {
                                tracing::error!(error = %e, "send_message: create_response failed");
                                return Err(e.into());
                            }
                        }
                        match cmd.get_response(&self.http).await {
                            Ok(msg) => {
                                tracing::debug!(message.id = %msg.id, "send_message: got response id");
                                return Ok(msg.id);
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "send_message: get_response failed");
                                return Err(e.into());
                            }
                        }
                    }
                }
                DEFERRED => {
                    // Was deferred — edit the original response to replace
                    // the "thinking…" indicator with actual content.
                    let mut builder = poise::serenity_prelude::EditInteractionResponse::new();
                    if let Some(ref content) = payload.content {
                        builder = builder.content(content);
                    }
                    if let Some(ref embed) = payload.embed {
                        builder = builder.embeds(vec![embed.clone()]);
                    }
                    cmd.edit_response(&self.http, builder).await?;
                    return Ok(cmd.get_response(&self.http).await?.id);
                }
                _ => {}
            }

            // Already responded (full message) — send as a followup.
            let mut followup = CreateInteractionResponseFollowup::new();
            if let Some(ref content) = payload.content {
                followup = followup.content(content);
            }
            if let Some(ref embed) = payload.embed {
                followup = followup.embeds(vec![embed.clone()]);
            }
            if payload.ephemeral {
                followup = followup.ephemeral(true);
            }
            Ok(cmd.create_followup(&self.http, followup).await?.id)
        } else {
            // Prefix context — send a channel message
            let mut msg = CreateMessage::new();
            if let Some(ref content) = payload.content {
                msg = msg.content(content);
            }
            if let Some(ref embed) = payload.embed {
                msg = msg.embed(embed.clone());
            }
            Ok(self
                .http
                .send_message(GenericChannelId::new(self.channel_id), vec![], &msg)
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
            let mut builder = EditInteractionResponse::new();
            if let Some(ref content) = payload.content {
                builder = builder.content(content);
            }
            if let Some(ref embed) = payload.embed {
                builder = builder.embeds(vec![embed.clone()]);
            }
            cmd.edit_response(&self.http, builder).await?;
        } else {
            let mut builder = EditMessage::new();
            if let Some(ref content) = payload.content {
                builder = builder.content(content);
            }
            if let Some(ref embed) = payload.embed {
                builder = builder.embed(embed.clone());
            }
            self.http
                .edit_message(
                    GenericChannelId::new(self.channel_id),
                    message_id,
                    &builder,
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
