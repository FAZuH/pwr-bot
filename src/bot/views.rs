//! Discord Components V2 view system.
//!
//! Provides traits and utilities for building interactive UI components.

use std::collections::HashMap;
use std::time::Duration;

use poise::CreateReply;
use poise::ReplyHandle;
use serenity::all::ComponentInteraction;
use serenity::all::ComponentInteractionCollector;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;
use serenity::all::CreateEmbed;
use serenity::all::CreateInteractionResponse;
use serenity::all::CreateSelectMenu;
use serenity::all::CreateSelectMenuKind;
use serenity::all::CreateSelectMenuOption;
use serenity::all::MessageFlags;
use serenity::all::MessageId;

use crate::bot::commands::Context;
use crate::bot::commands::Error;

pub mod pagination;

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

impl<'a> From<ResponseKind<'a>> for CreateReply<'a> {
    fn from(value: ResponseKind<'a>) -> Self {
        match value {
            ResponseKind::Component(components) => CreateReply::new()
                .flags(MessageFlags::IS_COMPONENTS_V2)
                .components(components),
            ResponseKind::Embed(embed) => CreateReply::new().embed(*embed),
        }
    }
}

/// Context data for views.
pub struct ViewContext<'a> {
    pub poise_ctx: &'a Context<'a>,
    pub timeout: Duration,
    pub reply_handle: Option<ReplyHandle<'a>>,
}

impl<'a> ViewContext<'a> {
    /// Creates a new view context with default data.
    pub fn new(ctx: &'a Context<'a>, timeout: Duration) -> Self {
        Self {
            poise_ctx: ctx,
            timeout,
            reply_handle: None,
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

/// Registry for actions that maps unique IDs to action instances.
///
/// This struct manages the mapping between Discord custom_id strings
/// and action instances, allowing views to retrieve the original action
/// from an interaction without recreating it.
pub struct ActionRegistry<T> {
    actions: HashMap<String, T>,
    prefix: String,
    counter: usize,
}

impl<T> ActionRegistry<T> {
    /// Creates a new empty action registry with an auto-generated unique prefix.
    ///
    /// The prefix combines the type name and a timestamp to ensure uniqueness
    /// across different view instances while remaining readable.
    pub fn new() -> Self {
        let type_name = std::any::type_name::<T>();
        let type_name = type_name.rsplit("::").next().unwrap_or(type_name);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let prefix = format!("{}:{}", type_name, timestamp);
        Self {
            actions: HashMap::new(),
            prefix,
            counter: 0,
        }
    }

    /// Registers an action and returns a unique ID for it.
    ///
    /// The returned ID should be used as the `custom_id` for Discord components.
    /// When an interaction comes back with this ID, use `get()` to retrieve
    /// the original action instance.
    pub fn register(&mut self, action: T) -> String {
        let id = format!("{}:{}", self.prefix, self.counter);
        self.counter += 1;
        self.actions.insert(id.clone(), action);
        id
    }

    /// Gets an action by its ID.
    pub fn get(&self, id: &str) -> Option<&T> {
        self.actions.get(id)
    }

    /// Returns all registered IDs.
    pub fn ids(&self) -> Vec<&str> {
        self.actions.keys().map(|s| s.as_str()).collect()
    }

    /// Checks if the registry contains the given ID.
    pub fn contains(&self, id: &str) -> bool {
        self.actions.contains_key(id)
    }

    pub fn clear(&mut self) {
        self.actions.clear();
    }
}

/// Core view primitive that provides Discord I/O operations.
///
/// This is a low-level building block. Views should embed this and provide
/// higher-level orchestration methods.
pub struct ViewCore<'a, T = ()> {
    pub ctx: ViewContext<'a>,
    pub registry: ActionRegistry<T>,
}

impl<'a, T> ViewCore<'a, T> {
    /// Creates a new view core.
    pub fn new(ctx: &'a Context<'a>, timeout: Duration) -> Self {
        Self {
            ctx: ViewContext::new(ctx, timeout),
            registry: ActionRegistry::new(),
        }
    }

    /// Sends a response as a new message.
    pub async fn send<'b>(&mut self, reply: CreateReply<'b>) -> Result<(), Error> {
        let handle = self.ctx.poise_ctx.send(reply).await?;
        self.ctx.reply_handle = Some(handle);
        Ok(())
    }

    /// Edits the stored message with a response.
    pub async fn edit(&self, reply: CreateReply<'a>) -> Result<(), Error> {
        if let Some(handle) = &self.ctx.reply_handle {
            handle.edit(*self.ctx.poise_ctx, reply).await?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
pub trait View<'a, T = ()> {
    fn core(&self) -> &ViewCore<'a, T>;
    fn core_mut(&mut self) -> &mut ViewCore<'a, T>;
    fn create_core(poise_ctx: &'a Context<'a>) -> ViewCore<'a, T>;
}

#[async_trait::async_trait]
pub trait ResponseView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b>;
    fn create_reply<'b>(&mut self) -> CreateReply<'b> {
        self.create_response().into()
    }
}

#[async_trait::async_trait]
pub trait RenderExt<'a, T> {
    /// Render content if not already rendered, otherwise edit the existing render with new render.
    async fn render(&mut self) -> Result<(), Error>;
}

#[async_trait::async_trait]
impl<'a, T, S> RenderExt<'a, T> for S
where
    S: View<'a, T> + ResponseView<'a> + Send,
    T: Action + Send + Sync + 'a,
{
    async fn render(&mut self) -> Result<(), Error> {
        let reply = self.create_reply();
        if let Some(handle) = &self.core().ctx.reply_handle {
            handle.edit(*self.core().ctx.poise_ctx, reply).await?;
        } else {
            let handle = self.core_mut().ctx.poise_ctx.send(reply).await?;
            self.core_mut().ctx.reply_handle = Some(handle);
        }
        Ok(())
    }
}

pub struct RegisteredAction {
    pub id: String,
    pub label: &'static str,
}

impl RegisteredAction {
    pub fn as_button(self) -> CreateButton<'static> {
        CreateButton::new(self.id).label(self.label)
    }

    pub fn as_select<'a>(self, kind: CreateSelectMenuKind<'a>) -> CreateSelectMenu<'a> {
        CreateSelectMenu::new(self.id, kind)
    }

    pub fn as_select_option(self) -> CreateSelectMenuOption<'static> {
        CreateSelectMenuOption::new(self.label, self.id)
    }
}

use futures::future::BoxFuture;

/// Enum returned by controller callbacks to signal the next action for the view loop.
pub enum ViewCommand {
    /// Re-render the view and continue listening.
    Render,
    /// Exit the view loop.
    Exit,
    /// Ignore the event and continue listening without re-rendering.
    Ignore,
}

/// Trait implemented by views to handle interactions and timeouts.
#[async_trait::async_trait]
pub trait ViewHandler<T: Action>: Send + Sync {
    /// Handles an action and returns the next action if any.
    async fn handle(
        &mut self,
        action: &T,
        interaction: &ComponentInteraction,
    ) -> Result<Option<T>, Error>;

    /// Callback to execute when the interaction is timed out.
    async fn on_timeout(&mut self) -> Result<(), Error> {
        Ok(())
    }

    /// Provides child view resolvers for action routing.
    fn children(&mut self) -> Vec<Box<dyn ChildViewResolver<T> + '_>> {
        vec![]
    }
}

/// Base implementation of interactive view logic.
pub struct InteractiveViewBase<'a, T: Action + 'a> {
    pub core: ViewCore<'a, T>,
    pub should_acknowledge: bool,
}

impl<'a, T: Action + 'a> InteractiveViewBase<'a, T> {
    pub fn new(core: ViewCore<'a, T>) -> Self {
        Self {
            core,
            should_acknowledge: true,
        }
    }

    /// Configures whether to automatically acknowledge interactions.
    pub fn acknowledge(mut self, should_acknowledge: bool) -> Self {
        self.should_acknowledge = should_acknowledge;
        self
    }

    /// Collects a single interaction and dispatches it to the handler.
    pub async fn listen_once<H: ViewHandler<T>>(
        &mut self,
        handler: &mut H,
    ) -> Result<Option<(T, ComponentInteraction)>, Error> {
        let mut collector = self.create_collector(handler).await;

        if let Some(msg_id) = self.core.ctx.message_id().await {
            collector = collector.message_id(msg_id);
        }

        let interaction = match collector.next().await {
            Some(i) => i,
            None => return Ok(None),
        };

        if self.should_acknowledge {
            interaction
                .create_response(
                    self.core.ctx.poise_ctx.http(),
                    CreateInteractionResponse::Acknowledge,
                )
                .await
                .ok();
        }

        let action = match self.resolve_action(&interaction.data.custom_id, handler) {
            Some(action) => action,
            None => return Ok(None),
        };

        let action = match handler.handle(&action, &interaction).await? {
            Some(action) => action,
            None => return Ok(None),
        };

        self.clear_registry(handler);
        Ok(Some((action, interaction)))
    }

    /// Registers an action and returns a unique ID for it.
    pub fn register(&mut self, action: T) -> RegisteredAction {
        let label = action.label();
        let id = self.core.registry.register(action);
        RegisteredAction { id, label }
    }

    async fn create_collector<H: ViewHandler<T>>(
        &mut self,
        handler: &mut H,
    ) -> ComponentInteractionCollector<'a> {
        let filter_ids = self.collector_filter(handler);
        ComponentInteractionCollector::new(self.core.ctx.poise_ctx.serenity_context())
            .author_id(self.core.ctx.poise_ctx.author().id)
            .timeout(self.core.ctx.timeout)
            .filter(move |i| filter_ids.contains(&i.data.custom_id.to_string()))
    }

    fn clear_registry<H: ViewHandler<T>>(&mut self, handler: &mut H) {
        self.core.registry.clear();
        for mut child in handler.children() {
            child.clear()
        }
    }

    fn collector_filter<H: ViewHandler<T>>(&mut self, handler: &mut H) -> Vec<String> {
        let mut ids: Vec<String> = self
            .core
            .registry
            .ids()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        for child in handler.children() {
            ids.extend(child.filter_ids());
        }
        ids
    }

    fn resolve_action<H: ViewHandler<T>>(&mut self, custom_id: &str, handler: &mut H) -> Option<T> {
        if let Some(action) = self.core.registry.get(custom_id) {
            return Some(action.clone());
        }
        for child in handler.children() {
            if let Some(action) = child.resolve(custom_id) {
                return Some(action);
            }
        }
        None
    }
}

/// Trait for interactive views that handle actions.
#[async_trait::async_trait]
pub trait InteractiveView<'a, T: Action + 'a>: ResponseView<'a> + Send + Sync {
    type Handler: ViewHandler<T> + Send + Sync;

    fn handler(&mut self) -> &mut Self::Handler;

    /// Runs the view event loop.
    async fn run<F>(&mut self, on_action: F) -> Result<(), Error>
    where
        F: FnMut(T) -> BoxFuture<'a, ViewCommand> + Send + Sync;
}

pub fn child<'a, 'b, C, S, T>(
    view: &'b mut S,
    wrap: fn(C) -> T,
) -> Box<dyn ChildViewResolver<T> + 'b>
where
    S: View<'a, C>,
    C: Action + Clone + 'a,
    T: Action + 'b,
    'a: 'b,
{
    Box::new((&mut view.core_mut().registry, wrap))
}

pub trait ChildViewResolver<T> {
    fn filter_ids(&self) -> Vec<String>;
    fn resolve(&self, custom_id: &str) -> Option<T>;
    fn clear(&mut self);
}

impl<T, C: Clone> ChildViewResolver<T> for (&mut ActionRegistry<C>, fn(C) -> T) {
    fn filter_ids(&self) -> Vec<String> {
        self.0.ids().into_iter().map(|s| s.to_string()).collect()
    }
    fn resolve(&self, custom_id: &str) -> Option<T> {
        self.0.get(custom_id).cloned().map(self.1)
    }
    fn clear(&mut self) {
        self.0.clear();
    }
}

/// Trait for action types used in interactive views.
///
/// Actions are registered in an ActionRegistry and mapped to unique IDs.
/// When an interaction comes in, the registry is used to look up the
/// original action instance.
pub trait Action: Send + Sync + Clone {
    /// Returns a human-readable label for this action.
    fn label(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Clone)]
    enum TestAction {
        First,
        Second,
        #[allow(dead_code)]
        Third,
    }

    impl Action for TestAction {
        fn label(&self) -> &'static str {
            match self {
                TestAction::First => "First",
                TestAction::Second => "Second",
                TestAction::Third => "Third",
            }
        }
    }

    #[test]
    fn test_action_registry_new() {
        let registry = ActionRegistry::<TestAction>::new();
        assert!(registry.actions.is_empty());
    }

    #[test]
    fn test_action_registry_register() {
        let mut registry = ActionRegistry::<TestAction>::new();

        let id1 = registry.register(TestAction::First);
        assert!(id1.starts_with("TestAction:"));

        let id2 = registry.register(TestAction::Second);
        assert_ne!(id1, id2);
    }
}

// ─── V2 Architecture ───────────────────────────────────────────────────────────

use futures::StreamExt;
use serenity::all::Message;
use serenity::all::ModalInteraction;
use tokio::sync::mpsc;

/// Represents an event that can wake up the View Loop.
pub enum ViewEvent<T> {
    Component(T, ComponentInteraction),
    Modal(T, ModalInteraction),
    Message(T, Message),
    Async(T),
    Timeout,
}

/// Passed to the handler so it knows WHAT triggered the action.
pub enum Trigger<'a> {
    Component(&'a ComponentInteraction),
    Modal(&'a ModalInteraction),
    Message(&'a Message),
    Async,
    Timeout,
}

/// Context passed to the handler.
pub struct ViewContextV2<'a, T> {
    pub poise: &'a Context<'a>,
    pub tx: mpsc::UnboundedSender<ViewEvent<T>>,
}

impl<'a, T: Action + Send + 'static> ViewContextV2<'a, T> {
    /// Ergonomic helper to spawn a background task that eventually produces a new action
    pub fn spawn<F>(&self, future: F)
    where
        F: std::future::Future<Output = Option<T>> + Send + 'static,
    {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Some(action) = future.await {
                let _ = tx.send(ViewEvent::Async(action));
            }
        });
    }
}

/// The trait for Rendering UI
pub trait ViewRenderV2<T: Action> {
    fn render(&self, registry: &mut ActionRegistry<T>) -> ResponseKind<'_>;

    // Allows attaching attachments or setting ephemeral state
    fn create_reply(&self, registry: &mut ActionRegistry<T>) -> CreateReply<'_> {
        self.render(registry).into()
    }
}

/// The trait for State Management & Logic
#[async_trait::async_trait]
pub trait ViewHandlerV2<T: Action>: Send + Sync {
    async fn handle(
        &mut self,
        action: T,
        trigger: Trigger<'_>,
        ctx: &ViewContextV2<'_, T>,
    ) -> Result<ViewCommand, Error>;

    async fn on_timeout(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

/// The Engine that drives the entire View.
pub struct ViewEngine<'a, T: Action + Send + Sync + 'static, H: ViewHandlerV2<T> + ViewRenderV2<T>>
{
    pub ctx: &'a Context<'a>,
    pub handler: H,
    pub registry: ActionRegistry<T>,
    pub timeout: Duration,
    pub should_acknowledge: bool,
    reply_handle: Option<ReplyHandle<'a>>,
}

impl<'a, T: Action + Send + Sync + 'static, H: ViewHandlerV2<T> + ViewRenderV2<T>>
    ViewEngine<'a, T, H>
{
    pub fn new(ctx: &'a Context<'a>, handler: H, timeout: Duration) -> Self {
        Self {
            ctx,
            handler,
            registry: ActionRegistry::new(),
            timeout,
            should_acknowledge: true,
            reply_handle: None,
        }
    }

    pub fn acknowledge(mut self, should_acknowledge: bool) -> Self {
        self.should_acknowledge = should_acknowledge;
        self
    }

    pub async fn render_view(&mut self) -> Result<(), Error> {
        self.registry.clear(); // Rebuild custom_ids
        let reply = self.handler.create_reply(&mut self.registry);

        if let Some(handle) = &self.reply_handle {
            handle.edit(*self.ctx, reply).await?;
        } else {
            let handle = self.ctx.send(reply).await?;
            self.reply_handle = Some(handle);
        }
        Ok(())
    }

    pub async fn run<F>(&mut self, mut on_action: F) -> Result<(), Error>
    where
        F: FnMut(T) -> BoxFuture<'a, ViewCommand> + Send + Sync,
    {
        self.render_view().await?;

        let (tx, mut rx) = mpsc::unbounded_channel::<ViewEvent<T>>();

        let msg_id = self.reply_handle.as_ref().unwrap().message().await?.id;
        let collector = ComponentInteractionCollector::new(self.ctx.serenity_context())
            .author_id(self.ctx.author().id)
            .message_id(msg_id)
            .timeout(self.timeout);

        // Convert the builder into a stream so we can poll it repeatedly
        let mut stream = collector.stream();

        let ctx_wrapper = ViewContextV2 {
            poise: self.ctx,
            tx: tx.clone(),
        };

        loop {
            tokio::select! {
                interaction = stream.next() => {
                    let interaction = match interaction {
                        Some(i) => i,
                        None => {
                            let _ = tx.send(ViewEvent::Timeout);
                            continue;
                        }
                    };


                    if self.should_acknowledge {
                        interaction.create_response(
                            self.ctx.http(),
                            CreateInteractionResponse::Acknowledge,
                        ).await.ok();
                    }

                    if let Some(action) = self.registry.get(&interaction.data.custom_id).cloned() {
                        let cmd = self.handler.handle(action.clone(), Trigger::Component(&interaction), &ctx_wrapper).await?;
                        if let ViewCommand::Render = cmd {
                            self.render_view().await?;
                        } else if let ViewCommand::Exit = cmd {
                            break;
                        }

                        let callback_cmd = on_action(action).await;
                        if let ViewCommand::Render = callback_cmd {
                            self.render_view().await?;
                        } else if let ViewCommand::Exit = callback_cmd {
                            break;
                        }
                    }
                }

                async_action = rx.recv() => {
                    let event = match async_action {
                        Some(e) => e,
                        None => break,
                    };

                    let mut action_taken = None;

                    let cmd = match event {
                        ViewEvent::Component(action, interaction) => {
                            action_taken = Some(action.clone());
                            self.handler.handle(action, Trigger::Component(&interaction), &ctx_wrapper).await?
                        }
                        ViewEvent::Modal(action, interaction) => {
                            action_taken = Some(action.clone());
                            self.handler.handle(action, Trigger::Modal(&interaction), &ctx_wrapper).await?
                        }
                        ViewEvent::Message(action, msg) => {
                            action_taken = Some(action.clone());
                            self.handler.handle(action, Trigger::Message(&msg), &ctx_wrapper).await?
                        }
                        ViewEvent::Async(action) => {
                            action_taken = Some(action.clone());
                            self.handler.handle(action, Trigger::Async, &ctx_wrapper).await?
                        }
                        ViewEvent::Timeout => {
                            self.handler.on_timeout().await?;
                            ViewCommand::Render
                        }
                    };

                    if let ViewCommand::Render = cmd {
                        self.render_view().await?;
                    } else if let ViewCommand::Exit = cmd {
                        break;
                    }

                    if let Some(action) = action_taken {
                        let callback_cmd = on_action(action).await;
                        if let ViewCommand::Render = callback_cmd {
                            self.render_view().await?;
                        } else if let ViewCommand::Exit = callback_cmd {
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
