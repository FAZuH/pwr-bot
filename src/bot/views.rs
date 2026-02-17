//! Discord Components V2 view system.
//!
//! Provides traits and utilities for building interactive UI components.

use std::str::FromStr;
use std::time::Duration;

use poise::CreateReply;
use poise::ReplyHandle;
use serenity::all::ComponentInteraction;
use serenity::all::ComponentInteractionCollector;
use serenity::all::CreateAttachment;
use serenity::all::CreateComponent;
use serenity::all::CreateEmbed;
use serenity::all::CreateInteractionResponse;
use serenity::all::MessageFlags;
use serenity::all::MessageId;

use crate::bot::commands::Context;
use crate::bot::commands::Error;

pub mod pagination;

/// Context data for stateful views.
pub struct ViewContext<'a, D = ()> {
    pub poise_ctx: &'a Context<'a>,
    pub timeout: Duration,
    pub reply_handle: Option<ReplyHandle<'a>>,
    pub data: D,
}

impl<'a, D> ViewContext<'a, D> {
    /// Creates a new view context with default data.
    pub fn new(ctx: &'a Context<'a>, timeout: Duration) -> Self
    where
        D: Default,
    {
        Self {
            poise_ctx: ctx,
            timeout,
            reply_handle: None,
            data: D::default(),
        }
    }

    /// Creates a new view context with the provided data.
    pub fn with_data(ctx: &'a Context<'a>, timeout: Duration, data: D) -> Self {
        Self {
            poise_ctx: ctx,
            timeout,
            reply_handle: None,
            data,
        }
    }

    /// Gets the message ID for filtering interactions, if available.
    pub async fn message_id(&self) -> Option<MessageId> {
        self.reply_handle
            .as_ref()?
            .message()
            .await
            .ok()
            .map(|m| m.id)
    }
}

/// Enum representing the type of response content.
pub enum ResponseKind<'a> {
    Component(Vec<CreateComponent<'a>>),
    Embed(Box<CreateEmbed<'a>>),
}

impl<'a> From<Vec<CreateComponent<'a>>> for ResponseKind<'a> {
    fn from(value: Vec<CreateComponent<'a>>) -> Self {
        ResponseKind::Component(value)
    }
}

impl<'a> From<CreateEmbed<'a>> for ResponseKind<'a> {
    fn from(value: CreateEmbed<'a>) -> Self {
        ResponseKind::Embed(Box::new(value))
    }
}

/// Trait for views that can provide response content.
pub trait ResponseProvider {
    /// Returns the response content for this view.
    fn create_response<'a>(&self) -> ResponseKind<'a>;

    /// Creates a reply based on the response kind.
    fn create_reply<'a>(&self) -> CreateReply<'a> {
        match self.create_response() {
            ResponseKind::Component(components) => CreateReply::new()
                .flags(MessageFlags::IS_COMPONENTS_V2)
                .components(components),
            ResponseKind::Embed(embed) => CreateReply::new().embed(*embed),
        }
    }
}

/// Trait for views with stored context and state.
#[async_trait::async_trait]
pub trait StatefulView<'a, D = ()>: ResponseProvider + Send + Sync
where
    D: Send + Sync + 'static,
{
    /// Returns a reference to the view context.
    fn view_context(&self) -> &ViewContext<'a, D>;

    /// Returns a mutable reference to the view context.
    fn view_context_mut(&mut self) -> &mut ViewContext<'a, D>;

    /// Sends this view as a new message and stores the reply handle.
    async fn send(&mut self) -> Result<(), Error> {
        let reply = self.create_reply();
        let ctx = self.view_context_mut();
        let handle = ctx.poise_ctx.send(reply).await?;
        ctx.reply_handle = Some(handle);
        Ok(())
    }

    /// Sends this view with an attachment and stores the reply handle.
    async fn send_with_attachment(
        &mut self,
        attachment: CreateAttachment<'a>,
    ) -> Result<(), Error> {
        let reply = self.create_reply().attachment(attachment);
        let ctx = self.view_context_mut();
        let handle = ctx.poise_ctx.send(reply).await?;
        ctx.reply_handle = Some(handle);
        Ok(())
    }

    /// Edits the stored reply with updated components.
    async fn edit(&self) -> Result<(), Error> {
        if let Some(handle) = &self.view_context().reply_handle {
            handle
                .edit(*self.view_context().poise_ctx, self.create_reply())
                .await?;
        }
        Ok(())
    }

    /// Edits the stored reply with updated components and an attachment.
    async fn edit_with_attachment(&self, attachment: CreateAttachment<'a>) -> Result<(), Error> {
        if let Some(handle) = &self.view_context().reply_handle {
            let reply = self.create_reply().attachment(attachment);
            handle.edit(*self.view_context().poise_ctx, reply).await?;
        }
        Ok(())
    }
}

/// Trait for views that handle component interactions.
#[async_trait::async_trait]
pub trait InteractableComponentView<'a, T, D = ()>: StatefulView<'a, D>
where
    for<'async_trait> T: Action + 'async_trait,
    D: Send + Sync + 'static,
{
    /// Handles an action and returns the next action if any.
    #[allow(unused_variables)]
    async fn handle_action(&mut self, action: T) -> Option<T> {
        None
    }

    /// Callback to execute when the interaction is timed out.
    #[allow(unused_variables)]
    async fn on_timeout(&mut self) -> Result<(), Error> {
        Ok(())
    }

    /// Handles an interaction and returns the action if recognized.
    async fn handle(&mut self, interaction: &ComponentInteraction) -> Option<T> {
        let action = Self::get_action(interaction)?;
        self.handle_action(action).await
    }

    /// Waits for a single interaction and handles it.
    async fn listen_once(&mut self) -> Result<Option<(T, ComponentInteraction)>, Error> {
        let ctx = self.view_context();
        let mut collector = Self::create_collector(ctx).await;

        // Filter by message ID if we have a reply handle
        if let Some(msg_id) = ctx.message_id().await {
            collector = collector.message_id(msg_id);
        }

        let interaction = match collector.next().await {
            Some(i) => i,
            None => {
                self.on_timeout().await?;
                return Ok(None);
            }
        };

        interaction
            .create_response(ctx.poise_ctx.http(), CreateInteractionResponse::Acknowledge)
            .await
            .ok();

        Ok(self
            .handle(&interaction)
            .await
            .map(|action| (action, interaction)))
    }

    /// Create a collector to collect this interaction
    async fn create_collector(ctx: &ViewContext<'a, D>) -> ComponentInteractionCollector<'a> {
        let filter_ids = Self::collector_custom_id_filter();
        let mut collector = ComponentInteractionCollector::new(ctx.poise_ctx.serenity_context())
            .author_id(ctx.poise_ctx.author().id)
            .timeout(ctx.timeout)
            .filter(move |i| filter_ids.contains(&i.data.custom_id.as_str()));

        if let Some(id) = ctx.message_id().await {
            collector = collector.message_id(id);
        }
        collector
    }

    /// Array or custom_id used to filter interactions
    fn collector_custom_id_filter() -> &'static [&'static str] {
        T::all()
    }

    /// Convert [`ComponentInteraction`] into [`T`]
    fn get_action(interaction: &ComponentInteraction) -> Option<T> {
        T::from_str(&interaction.data.custom_id).ok()
    }
}

/// Trait for action enums used in interactive views.
pub trait Action: FromStr + Send {
    /// All possible action strings.
    fn all() -> &'static [&'static str];

    /// Returns the custom_id for this action.
    fn custom_id(&self) -> &'static str;

    /// Returns a human-readable label for this action.
    fn label(&self) -> &'static str;
}
