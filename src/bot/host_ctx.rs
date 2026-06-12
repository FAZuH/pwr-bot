use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use poise::serenity_prelude::*;

use crate::bot::Data;
use crate::bot::command::Context as BotContext;
use crate::bot::command::Error;

/// Owned message payload for sending across trait boundaries.
pub struct MessagePayload {
    pub content: Option<String>,
    pub embed: Option<CreateEmbed<'static>>,
    pub ephemeral: bool,
}

impl MessagePayload {
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
    fn guild_id(&self) -> Option<u64>;
    fn author_id(&self) -> u64;
    fn channel_id(&self) -> u64;
    fn data(&self) -> Arc<Data>;

    async fn defer(&self) -> Result<(), Error>;
    async fn send_message(&self, payload: &MessagePayload) -> Result<MessageId, Error>;
    async fn edit_message(
        &self,
        message_id: MessageId,
        payload: &MessagePayload,
    ) -> Result<(), Error>;
    async fn acknowledge(&self, interaction: &ComponentInteraction) -> Result<(), Error>;
}

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
    /// Tracks whether we have sent an initial interaction response.
    responded: AtomicBool,
}

impl PoiseHostCtx {
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
            responded: AtomicBool::new(false),
        })
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
        if let Some(cmd) = self.cmd_interaction()
            && !self.responded.swap(true, Ordering::SeqCst)
        {
            cmd.defer(&self.http).await?;
        }
        Ok(())
    }

    async fn send_message(&self, payload: &MessagePayload) -> Result<MessageId, Error> {
        let builder = self.build_response_message(payload);

        if let Some(cmd) = self.cmd_interaction() {
            if !self.responded.swap(true, Ordering::SeqCst) {
                cmd.create_response(&self.http, CreateInteractionResponse::Message(builder))
                    .await?;
                Ok(cmd.get_response(&self.http).await?.id)
            } else {
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
            }
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
