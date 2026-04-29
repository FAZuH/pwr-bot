//! Discord Components V2 view system.
//!
//! Provides traits and utilities for building interactive UI components using a
//! View-Controller pattern. This system manages the lifecycle of Discord message
//! components (buttons, select menus) and handles user interactions via an event loop.
//!
//! ### Architecture Overview
//! The system is built around three core traits:
//! - [`Action`]: An enum representing all possible user interactions in the view.
//! - [`ViewRender`]: Defines how to translate state into Discord components/embeds.
//! - [`ViewHandler`]: Contains the business logic and state mutations for each action.
//!
//! These are orchestrated by the [`ViewEngine`], which runs an async event loop
//! processing interactions, manual events, and timeouts.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use poise::CreateReply;
use poise::serenity_prelude::*;
use tokio::sync::RwLock;
use tokio::sync::mpsc;

use crate::bot::command::Context;
use crate::bot::command::Error;
use crate::bot::coordinator::Coordinator;

/// Type alias for a thread-safe, shared handle to a Discord message.
type EventMessage<T> = (Option<T>, ViewEvent);
type Registry<T> = Arc<RwLock<ActionRegistry<T>>>;

pub mod pagination;

// ── Response Content ───────────────────────────────────────────────────────────────

/// Enum representing the type of response content.
///
/// This abstracts over whether a view is rendering a set of components (buttons/selects)
/// or a single embed.
pub enum ResponseKind<'a> {
    /// A set of message components (Buttons, Select Menus).
    Component(Vec<CreateComponent<'a>>),
    /// A single Discord embed.
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

// ── Actions ───────────────────────────────────────────────────────────────

/// Trait for action types used in interactive views.
pub trait Action: Send + Sync + Clone + std::fmt::Debug {
    /// Returns the UI label associated with this action.
    fn label(&self) -> &'static str;
}

/// Registry for actions that maps unique IDs to action instances.
pub struct ActionRegistry<T> {
    pub actions: HashMap<String, T>,
    prefix: String,
    counter: usize,
}

impl<T: Action> ActionRegistry<T> {
    /// Creates a new, empty registry with a unique prefix based on the type name and timestamp.
    pub fn new() -> Self {
        let type_name = std::any::type_name::<T>();
        let type_name = type_name.rsplit("::").next().unwrap_or(type_name);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        Self {
            actions: HashMap::new(),
            prefix: format!("{}:{}", type_name, timestamp),
            counter: 0,
        }
    }

    /// Registers an action and returns a [`RegisteredAction`] for building Discord components.
    pub fn register(&mut self, action: T) -> RegisteredAction {
        let id = format!("{}:{}", self.prefix, self.counter);
        let label = action.label();
        self.counter += 1;
        self.actions.insert(id.clone(), action);
        RegisteredAction { id, label }
    }

    /// Registers an action using given id, and returns a [`RegisteredAction`] for building Discord components.
    ///
    /// If an action is already registered with the same id, the action will be updated, and the
    /// old action is returned.
    pub fn register_with_id(&mut self, id: &str, action: T) -> Option<RegisteredAction> {
        self.actions
            .insert(id.to_string(), action)
            .map(|old| RegisteredAction {
                id: id.to_string(),
                label: old.label(),
            })
    }

    /// Retrieves an action associated with a given `custom_id`.
    pub fn get(&self, id: &str) -> Option<&T> {
        self.actions.get(id)
    }

    /// Clears all registered actions. Called before re-rendering.
    pub fn clear(&mut self) {
        self.actions.clear();
    }
}

impl<T: Action> Default for ActionRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A registered action paired with its UI label, used to build Discord components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredAction {
    /// The unique custom ID generated by the registry.
    pub id: String,
    /// The display label for the component.
    pub label: &'static str,
}

impl RegisteredAction {
    /// Converts the registered action into a Discord button.
    pub fn as_button(self) -> CreateButton<'static> {
        CreateButton::new(self.id).label(self.label)
    }

    /// Converts the registered action into a Discord select menu.
    pub fn as_select<'a>(self, kind: CreateSelectMenuKind<'a>) -> CreateSelectMenu<'a> {
        CreateSelectMenu::new(self.id, kind)
    }

    /// Converts the registered action into a Discord select menu option.
    pub fn as_select_option(self) -> CreateSelectMenuOption<'static> {
        CreateSelectMenuOption::new(self.label, self.id)
    }
}

// ── View Enums ───────────────────────────────────────────────────────────────

/// Returned by [`ViewHandler::handle`] to control the engine loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewCommand {
    /// Re-render the view and update the Discord message.
    Render,
    /// Render once then exit the view loop immediately.
    RenderOnce,
    /// Exit the view loop immediately.
    Exit,
    /// Continue the loop without re-rendering.
    Continue,
    /// The interaction was already responded to (e.g. a modal was opened).
    /// The engine will skip auto-acknowledgement.
    AlreadyResponded,
}

/// Values extracted from a select menu interaction.
#[derive(Debug, Clone)]
pub enum SelectValues {
    String(Vec<String>),
    Channel(Vec<GenericChannelId>),
    Role(Vec<RoleId>),
    User(Vec<UserId>),
}

/// A synthetic event used for automated GUI testing.
#[derive(Debug, Clone)]
pub enum SyntheticEvent {
    Button,
    Select(SelectValues),
}

/// An event that wakes up the [`ViewEngine`] loop.
///
/// Replaces the former separate `Trigger` enum — the action and its originating
/// interaction are kept together, eliminating redundancy with [`ViewContext`].
#[derive(Debug, Clone)]
pub enum ViewEvent {
    /// A component interaction (button click, select menu choice).
    Component(ComponentInteraction),
    /// A modal submission.
    Modal(ModalInteraction),
    /// A message in the channel.
    Message(Message),
    /// A reaction to a listened message.
    Reaction(Reaction),
    /// An async event triggered via [`ViewContext::spawn`].
    Async,
    /// The view loop timed out.
    Timeout,
    /// A synthetic event injected by the test framework.
    Synthetic(SyntheticEvent),
}

// ── View Sender ───────────────────────────────────────────────────────────────

/// Trait for sending events to a running [`ViewEngine`].
pub trait ViewSender<T: Action>: Send + Sync {
    fn send(&self, message: EventMessage<T>);
}

impl<T: Action> ViewSender<T> for mpsc::UnboundedSender<EventMessage<T>> {
    fn send(&self, message: EventMessage<T>) {
        let _ = self.send(message);
    }
}

/// Maps child actions of type `C` to parent actions of type `P`.
///
/// Enables the delegation pattern where a child view's interactions are
/// forwarded to the parent's action enum.
pub struct MappedViewSender<C: Action, P: Action> {
    parent_tx: Arc<dyn ViewSender<P>>,
    wrap: fn(Option<C>) -> Option<P>,
}

impl<C: Action, P: Action> ViewSender<C> for MappedViewSender<C, P> {
    fn send(&self, message: EventMessage<C>) {
        let (action, event) = message;
        let new_action = (self.wrap)(action);
        self.parent_tx.send((new_action, event));
    }
}

// ── ViewChannel ───────────────────────────────────────────────────────────────

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
/// Configuration for [`ViewChannel`] about what events is collected.
pub struct ViewChannelConfig {
    pub components: bool,
    pub modals: bool,
    pub messages: bool,
    pub reactions: bool,
}

impl Default for ViewChannelConfig {
    fn default() -> Self {
        Self {
            components: true, // What this bot builds most of its responses on
            modals: false,
            messages: false,
            reactions: false,
        }
    }
}

pub struct ViewChannel<T: Action + Send + Sync + 'static> {
    tx: mpsc::UnboundedSender<EventMessage<T>>,
    rx: mpsc::UnboundedReceiver<EventMessage<T>>,
    /// Snapshot of custom_id → action, updated after every render_view().
    registry: Registry<T>,
    config: ViewChannelConfig,
}

impl<T: Action + Send + Sync + 'static> ViewChannel<T> {
    pub fn new(config: ViewChannelConfig, registry: Registry<T>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            tx,
            rx,
            registry,
            config,
        }
    }

    pub fn sender(&self) -> Arc<dyn ViewSender<T>> {
        Arc::new(self.tx.clone())
    }

    pub async fn recv(&mut self) -> Option<EventMessage<T>> {
        self.rx.recv().await
    }

    /// Spawns all enabled collectors into background tasks.
    /// Must be called after the first render_view() so msg_id is available.
    pub fn start(
        &self,
        ctx: &Context<'_>,
        msg_id: MessageId,
        author_id: UserId,
        channel_id: GenericChannelId,
        timeout: Duration,
    ) {
        let serenity_ctx = ctx.serenity_context().clone();

        if self.config.components {
            let tx = self.tx.clone();
            let registry = self.registry.clone();
            let sctx = serenity_ctx.clone();
            tokio::spawn(async move {
                let collector = ComponentInteractionCollector::new(&sctx)
                    .author_id(author_id)
                    .message_id(msg_id)
                    .timeout(timeout);
                let mut stream = collector.stream();
                while let Some(interaction) = stream.next().await {
                    let action = registry
                        .read()
                        .await
                        .get(interaction.data.custom_id.as_ref())
                        .cloned();
                    let _ = tx.send((action, ViewEvent::Component(interaction.clone())));
                }
                let _ = tx.send((None, ViewEvent::Timeout));
            });
        }

        if self.config.modals {
            let tx = self.tx.clone();
            let sctx = serenity_ctx.clone();
            let registry = self.registry.clone();
            tokio::spawn(async move {
                let collector = ModalInteractionCollector::new(&sctx)
                    .author_id(author_id)
                    .timeout(timeout);
                let mut stream = collector.stream();
                while let Some(interaction) = stream.next().await {
                    let action = registry
                        .read()
                        .await
                        .get(interaction.data.custom_id.as_ref())
                        .cloned();
                    let _ = tx.send((action, ViewEvent::Modal(interaction.clone())));
                }
            });
        }

        if self.config.messages {
            let tx = self.tx.clone();
            let sctx = serenity_ctx.clone();
            tokio::spawn(async move {
                let collector = MessageCollector::new(&sctx)
                    .author_id(author_id)
                    .channel_id(channel_id)
                    .timeout(timeout);
                let mut stream = collector.stream();
                while let Some(msg) = stream.next().await {
                    let _ = tx.send((None, ViewEvent::Message(msg.clone())));
                }
            });
        }

        if self.config.reactions {
            let tx = self.tx.clone();
            let sctx = serenity_ctx.clone();
            tokio::spawn(async move {
                let collector = ReactionCollector::new(&sctx)
                    .author_id(author_id)
                    .message_id(msg_id)
                    .timeout(timeout);
                let mut stream = collector.stream();
                while let Some(reaction) = stream.next().await {
                    let _ = tx.send((None, ViewEvent::Reaction(reaction.clone())));
                }
            });
        }
    }
}
// ── ViewContext ───────────────────────────────────────────────────────────────

/// Passed to [`ViewHandler::handle`] for every non-timeout event.
///
/// Consolidates the Poise context, the event (action + interaction), the async
/// sender, and the shared coordinator into a single parameter.
pub struct ViewContext<'a, T: Action> {
    /// Poise command context.
    pub poise: Context<'a>,
    // None for Modal, Message, Reaction
    pub action: Option<T>,
    /// The event that triggered this handler call, including the action and
    /// any associated Discord interaction.
    pub event: ViewEvent,
    /// Sender for dispatching further events back to the engine loop.
    pub tx: Arc<dyn ViewSender<T>>,
    /// Shared coordinator — provides access to the reply handle and nav state.
    pub coordinator: Arc<Coordinator<'a>>,
}

impl<'a, T: Action + 'static> ViewContext<'a, T> {
    /// Returns a reference to the action that triggered this call.
    pub fn action(&self) -> &T {
        self.action
            .as_ref()
            .expect("ViewContext::action called on a Timeout event")
    }

    /// Extracts select-menu values from the event, if any.
    pub fn select_values(&self) -> Option<SelectValues> {
        match &self.event {
            ViewEvent::Component(interaction) => match &interaction.data.kind {
                ComponentInteractionDataKind::StringSelect { values } => {
                    Some(SelectValues::String(values.to_vec()))
                }
                ComponentInteractionDataKind::ChannelSelect { values } => {
                    Some(SelectValues::Channel(
                        values.iter().copied().map(GenericChannelId::from).collect(),
                    ))
                }
                ComponentInteractionDataKind::RoleSelect { values } => {
                    Some(SelectValues::Role(values.to_vec()))
                }
                ComponentInteractionDataKind::UserSelect { values } => {
                    Some(SelectValues::User(values.to_vec()))
                }
                _ => None,
            },
            ViewEvent::Synthetic(SyntheticEvent::Select(values)) => Some(values.clone()),
            _ => None,
        }
    }

    /// Returns string-select values, if this event was a string select.
    pub fn string_select_values(&self) -> Option<Vec<String>> {
        match self.select_values()? {
            SelectValues::String(v) => Some(v),
            _ => None,
        }
    }

    /// Returns channel-select values, if this event was a channel select.
    pub fn channel_select_values(&self) -> Option<Vec<GenericChannelId>> {
        match self.select_values()? {
            SelectValues::Channel(v) => Some(v),
            _ => None,
        }
    }

    /// Returns role-select values, if this event was a role select.
    pub fn role_select_values(&self) -> Option<Vec<RoleId>> {
        match self.select_values()? {
            SelectValues::Role(v) => Some(v),
            _ => None,
        }
    }

    /// Returns user-select values, if this event was a user select.
    pub fn user_select_values(&self) -> Option<Vec<UserId>> {
        match self.select_values()? {
            SelectValues::User(v) => Some(v),
            _ => None,
        }
    }

    /// Creates a child context that maps child actions into the parent's action type.
    pub fn map<C: Action + Send + 'static>(
        &self,
        action: C,
        wrap: fn(Option<C>) -> Option<T>,
    ) -> ViewContext<'a, C> {
        ViewContext {
            poise: self.poise,
            action: Some(action),
            event: self.event.clone(),
            tx: Arc::new(MappedViewSender {
                parent_tx: self.tx.clone(),
                wrap,
            }),
            coordinator: self.coordinator.clone(),
        }
    }

    /// Spawns an async task that sends an action back to the engine on completion.
    pub fn spawn<F>(&self, future: F)
    where
        F: std::future::Future<Output = Option<T>> + Send + 'static,
    {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Some(action) = future.await {
                tx.send((Some(action), ViewEvent::Async));
            }
        });
    }
}

/// Defines how a view translates its state into Discord components or an embed.
pub trait ViewRender {
    type Action: Action;
    fn render(&self, registry: &mut ActionRegistry<Self::Action>) -> ResponseKind<'_>;

    fn create_reply(&self, registry: &mut ActionRegistry<Self::Action>) -> CreateReply<'_> {
        self.render(registry).into()
    }
}

/// Manages state and processes interactions for a view.
#[async_trait::async_trait]
pub trait ViewHandler: Send + Sync {
    type Action: Action;
    /// Handles a non-timeout event.
    ///
    /// Receives the full [`ViewContext`] containing the event, action, sender,
    /// and coordinator. Returns a [`ViewCommand`] controlling the engine loop.
    async fn handle(&mut self, ctx: ViewContext<'_, Self::Action>) -> Result<ViewCommand, Error>;

    /// Called when the view loop times out waiting for user input.
    async fn on_timeout(&mut self) -> Result<ViewCommand, Error> {
        Ok(ViewCommand::Exit)
    }

    /// The channel config this view sets
    fn channel_config(&self) -> ViewChannelConfig {
        ViewChannelConfig::default()
    }
}

/// The engine that drives the entire View lifecycle.
///
/// The `ViewEngine` performs the following cycle:
/// 1. Calls `render()` to display the initial UI.
/// 2. Enters a `tokio::select!` loop waiting for Discord interactions or async events.
/// 3. Matches interactions back to [`Action`] variants using the [`ActionRegistry`].
/// 4. Dispatches the action to the [`ViewHandler`].
/// 5. Reacts to the returned [`ViewCommand`] (Render, Exit, etc.).
pub struct ViewEngine<'a, T, H>
where
    T: Action + Send + Sync + 'static,
    H: ViewHandler<Action = T> + ViewRender<Action = T>,
{
    /// The combined handler and renderer.
    pub handler: H,
    /// Whether the engine should auto-acknowledge component interactions.
    should_acknowledge: bool,
    /// Poise command context.
    ctx: Context<'a>,
    /// Registry for mapping custom IDs to actions.
    registry: Registry<T>,
    /// Inactivity timeout for the interaction collector.
    timeout: Duration,
    /// Shared handle to the active message.
    coordinator: Arc<Coordinator<'a>>,
}

impl<'a, T, H> ViewEngine<'a, T, H>
where
    T: Action + Send + Sync + 'static,
    H: ViewHandler<Action = T> + ViewRender<Action = T>,
{
    pub fn new(
        ctx: Context<'a>,
        handler: H,
        timeout: Duration,
        coordinator: Arc<Coordinator<'a>>,
    ) -> Self {
        Self {
            ctx,
            handler,
            registry: Arc::new(RwLock::new(ActionRegistry::new())),
            timeout,
            should_acknowledge: true,
            coordinator,
        }
    }

    /// Sets whether the engine auto-acknowledges component interactions.
    /// Set to `false` when the handler opens a modal and responds manually.
    pub fn acknowledge(mut self, should_acknowledge: bool) -> Self {
        self.should_acknowledge = should_acknowledge;
        self
    }

    /// Starts the interactive event loop.
    pub async fn run(&mut self) -> Result<(), Error> {
        let mut channel = ViewChannel::new(self.handler.channel_config(), self.registry.clone());
        self.render_view().await?;

        let msg_id = {
            let lock = self.coordinator.reply_handle().await;
            lock.as_ref()
                .expect("reply_handle must exist after render_view")
                .message()
                .await?
                .id
        };

        channel.start(
            &self.ctx,
            msg_id,
            self.ctx.author().id,
            self.ctx.channel_id(),
            self.timeout,
        );

        let poise = self.ctx;
        let coordinator = self.coordinator.clone();
        let tx_arc = channel.sender();

        use ViewCommand::*;
        while let Some((action, event)) = channel.recv().await {
            let cmd = match event {
                ViewEvent::Timeout => self.handler.on_timeout().await?,
                ViewEvent::Component(ref interaction) => {
                    let Some(action) = action else {
                        if self.should_acknowledge {
                            interaction
                                .create_response(
                                    poise.http(),
                                    CreateInteractionResponse::Acknowledge,
                                )
                                .await
                                .ok();
                        }
                        continue;
                    };
                    // need to acknowledge after handle
                    let raw = interaction.clone();
                    let view_ctx = ViewContext {
                        action: Some(action),
                        poise,
                        event,
                        tx: tx_arc.clone(),
                        coordinator: coordinator.clone(),
                    };
                    let cmd = self.handler.handle(view_ctx).await?;
                    if self.should_acknowledge && !matches!(cmd, AlreadyResponded) {
                        raw.create_response(poise.http(), CreateInteractionResponse::Acknowledge)
                            .await
                            .ok();
                    }
                    cmd
                }
                other => {
                    let view_ctx = ViewContext {
                        action,
                        poise,
                        event: other,
                        tx: tx_arc.clone(),
                        coordinator: coordinator.clone(),
                    };
                    self.handler.handle(view_ctx).await?
                }
            };

            match cmd {
                Render => {
                    self.render_view().await?;
                }
                RenderOnce => {
                    self.render_view().await?;
                    break;
                }
                Exit => break,
                Continue | AlreadyResponded => {}
            }
        }

        Ok(())
    }

    /// Re-renders the view, editing the existing message or sending a new one.
    async fn render_view(&self) -> Result<(), Error> {
        let mut registry = self.registry.write().await;
        registry.clear();
        let reply = self.handler.create_reply(&mut registry);

        let existing = { self.coordinator.reply_handle().await.as_ref().cloned() };

        if let Some(handle) = existing {
            handle.edit(self.ctx, reply).await?;
        } else {
            let handle = self.ctx.send(reply).await?;
            self.coordinator.set_reply_handle(handle).await;
        }

        Ok(())
    }
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
        assert!(id1.id.starts_with("TestAction:"));

        let id2 = registry.register(TestAction::Second);
        assert_ne!(id1, id2);
    }
}
