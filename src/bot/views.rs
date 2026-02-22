//! Discord Components V2 view system.
//!
//! Provides traits and utilities for building interactive UI components.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::future::BoxFuture;
use log::debug;
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
use serenity::all::Message;
use serenity::all::MessageFlags;
use serenity::all::ModalInteraction;
use tokio::sync::mpsc;

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

/// Registry for actions that maps unique IDs to action instances.
pub struct ActionRegistry<T> {
    actions: HashMap<String, T>,
    prefix: String,
    counter: usize,
}

impl<T> ActionRegistry<T> {
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

    pub fn register(&mut self, action: T) -> String {
        let id = format!("{}:{}", self.prefix, self.counter);
        self.counter += 1;
        self.actions.insert(id.clone(), action);
        id
    }

    pub fn get(&self, id: &str) -> Option<&T> {
        self.actions.get(id)
    }

    pub fn clear(&mut self) {
        self.actions.clear();
    }
}

impl<T> Default for ActionRegistry<T> {
    fn default() -> Self {
        Self::new()
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

/// Enum returned by controller callbacks to signal the next action for the view loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewCommand {
    /// Re-render the view and continue listening.
    Render,
    /// Exit the view loop.
    Exit,
    /// Continue the loop without re-rendering.
    Continue,
    /// Indicates the interaction was already responded to (e.g., a modal was opened),
    /// so the engine should NOT auto-acknowledge it.
    AlreadyResponded,
}

/// Trait for action types used in interactive views.
pub trait Action: Send + Sync + Clone {
    fn label(&self) -> &'static str;
}

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

pub trait ViewSender<T: Action>: Send + Sync {
    fn send(&self, event: ViewEvent<T>);
}

impl<T: Action> ViewSender<T> for mpsc::UnboundedSender<ViewEvent<T>> {
    fn send(&self, event: ViewEvent<T>) {
        let _ = self.send(event);
    }
}

pub struct MappedSender<C: Action, P: Action> {
    parent_tx: Arc<dyn ViewSender<P>>,
    wrap: fn(C) -> P,
}

impl<C: Action, P: Action> ViewSender<C> for MappedSender<C, P> {
    fn send(&self, event: ViewEvent<C>) {
        let mapped_event = match event {
            ViewEvent::Component(c, i) => ViewEvent::Component((self.wrap)(c), i),
            ViewEvent::Modal(c, i) => ViewEvent::Modal((self.wrap)(c), i),
            ViewEvent::Message(c, m) => ViewEvent::Message((self.wrap)(c), m),
            ViewEvent::Async(c) => ViewEvent::Async((self.wrap)(c)),
            ViewEvent::Timeout => ViewEvent::Timeout,
        };
        self.parent_tx.send(mapped_event);
    }
}

/// Context passed to the handler.
pub struct ViewContext<'a, T> {
    pub poise: &'a Context<'a>,
    pub tx: Arc<dyn ViewSender<T>>,
}

impl<'a, T: Action + Send + 'static> ViewContext<'a, T> {
    pub fn map<C: Action + Send + 'static>(&self, wrap: fn(C) -> T) -> ViewContext<'a, C> {
        ViewContext {
            poise: self.poise,
            tx: Arc::new(MappedSender {
                parent_tx: self.tx.clone(),
                wrap,
            }),
        }
    }

    pub fn spawn<F>(&self, future: F)
    where
        F: std::future::Future<Output = Option<T>> + Send + 'static,
    {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Some(action) = future.await {
                tx.send(ViewEvent::Async(action));
            }
        });
    }
}

/// The trait for Rendering UI
pub trait ViewRender<T: Action> {
    fn render(&self, registry: &mut ActionRegistry<T>) -> ResponseKind<'_>;

    fn create_reply(&self, registry: &mut ActionRegistry<T>) -> CreateReply<'_> {
        self.render(registry).into()
    }
}

/// The trait for State Management & Logic
#[async_trait::async_trait]
pub trait ViewHandler<T: Action>: Send + Sync {
    async fn handle(
        &mut self,
        action: T,
        trigger: Trigger<'_>,
        ctx: &ViewContext<'_, T>,
    ) -> Result<ViewCommand, Error>;

    async fn on_timeout(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

/// The Engine that drives the entire View.
pub struct ViewEngine<'a, T, H>
where
    T: Action + Send + Sync + 'static,
    H: ViewHandler<T> + ViewRender<T>,
{
    pub ctx: &'a Context<'a>,
    pub handler: H,
    pub registry: ActionRegistry<T>,
    pub timeout: Duration,
    pub should_acknowledge: bool,
    reply_handle: Option<ReplyHandle<'a>>,
}

impl<'a, T, H> ViewEngine<'a, T, H>
where
    T: Action + Send + Sync + 'static,
    H: ViewHandler<T> + ViewRender<T>,
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
        self.registry.clear();
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

        let mut stream = collector.stream();

        let ctx_wrapper = ViewContext {
            poise: self.ctx,
            tx: Arc::new(tx.clone()),
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

                    if let Some(action) = self.registry.get(&interaction.data.custom_id).cloned() {
                        let cmd = self.handler.handle(action.clone(), Trigger::Component(&interaction), &ctx_wrapper).await?;
                        debug!("Handle returned {:?}", cmd);

                        if self.should_acknowledge && !matches!(cmd, ViewCommand::AlreadyResponded) {
                            interaction.create_response(
                                self.ctx.http(),
                                CreateInteractionResponse::Acknowledge,
                            ).await.ok();
                        }

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
                    } else if self.should_acknowledge {
                        interaction.create_response(
                            self.ctx.http(),
                            CreateInteractionResponse::Acknowledge,
                        ).await.ok();
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
