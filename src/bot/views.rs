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
    T: Action,
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

/// Trait for interactive views that handle actions.
#[async_trait::async_trait]
pub trait InteractiveView<'a, T: Action + 'a>: View<'a, T> + Sync {
    /// Handles an action and returns the next action if any.
    async fn handle(&mut self, action: &T, interaction: &ComponentInteraction) -> Option<T>;

    /// Callback to execute when the interaction is timed out.
    async fn on_timeout(&mut self) -> Result<(), Error> {
        Ok(())
    }

    /// Collects a single interaction.
    /// Returns the action and interaction if received, None if timed out.
    async fn listen_once(&mut self) -> Result<Option<(T, ComponentInteraction)>, Error> {
        let mut collector = self.create_collector().await;

        // Filter by message ID if we have a reply handle
        if let Some(msg_id) = self.core().ctx.message_id().await {
            collector = collector.message_id(msg_id);
        }

        let interaction = match collector.next().await {
            Some(i) => i,
            None => return Ok(None),
        };

        if Self::should_acknowledge() {
            interaction
                .create_response(
                    self.core().ctx.poise_ctx.http(),
                    CreateInteractionResponse::Acknowledge,
                )
                .await
                .ok();
        }

        let action = match self.resolve_action(&interaction.data.custom_id) {
            Some(action) => action,
            None => return Ok(None),
        };

        let action = match self.handle(&action, &interaction).await {
            Some(action) => action,
            None => return Ok(None),
        };

        self.clear_registry();

        Ok(Some((action, interaction)))
    }

    fn register(&mut self, action: T) -> RegisteredAction {
        let label = action.label();
        let id = self.core_mut().registry.register(action);
        RegisteredAction { id, label }
    }

    /// Creates a collector for interactions.
    async fn create_collector(&mut self) -> ComponentInteractionCollector<'a> {
        let filter_ids = self.collector_filter();
        let core = self.core();
        ComponentInteractionCollector::new(core.ctx.poise_ctx.serenity_context())
            .author_id(core.ctx.poise_ctx.author().id)
            .timeout(core.ctx.timeout)
            .filter(move |i| filter_ids.contains(&i.data.custom_id.to_string()))
    }

    fn children(&mut self) -> Vec<Box<dyn ChildViewResolver<T> + '_>> {
        vec![]
    }

    fn should_acknowledge() -> bool {
        true
    }

    fn child<'b, C, S>(view: &'b mut S, wrap: fn(C) -> T) -> Box<dyn ChildViewResolver<T> + 'b>
    where
        S: View<'a, C>,
        C: Action + Clone + 'a,
        'a: 'b,
    {
        Box::new((&mut view.core_mut().registry, wrap))
    }

    fn clear_registry(&mut self) {
        self.core_mut().registry.clear();
        for mut child in self.children() {
            child.clear()
        }
    }

    fn collector_filter(&mut self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .core()
            .registry
            .ids()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        for child in self.children() {
            ids.extend(child.filter_ids());
        }
        ids
    }

    fn resolve_action(&mut self, custom_id: &str) -> Option<T> {
        if let Some(action) = self.core().registry.get(custom_id) {
            return Some(action.clone());
        }
        for child in self.children() {
            if let Some(action) = child.resolve(custom_id) {
                return Some(action);
            }
        }
        None
    }
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
