//! Discord Components V2 view system.
//!
//! Provides traits and utilities for building interactive UI components.

use std::str::FromStr;
use std::time::Duration;

use poise::CreateReply;
use poise::ReplyHandle;
use serenity::all::ComponentInteraction;
use serenity::all::ComponentInteractionCollector;
use serenity::all::CreateComponent;
use serenity::all::CreateInteractionResponse;
use serenity::all::CreateMessage;
use serenity::all::MessageFlags;

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
    pub async fn message_id(&self) -> Option<serenity::all::MessageId> {
        self.reply_handle
            .as_ref()?
            .message()
            .await
            .ok()
            .map(|m| m.id)
    }
}

/// Trait for types that can create Discord UI components.
pub trait ViewProvider<'a, T = CreateComponent<'a>> {
    /// Creates the components for this view.
    fn create(&self) -> Vec<T>;
}

/// Trait for views that can be sent as a response to a command.
pub trait ResponseComponentView {
    /// Creates the components for this view.
    fn create_components<'a>(&self) -> Vec<CreateComponent<'a>>;

    /// Creates a reply with this view's components.
    fn create_reply<'a>(&self) -> CreateReply<'a> {
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(self.create_components())
    }

    /// Creates a message with this view's components.
    fn create_message<'a>(&self) -> CreateMessage<'a> {
        CreateMessage::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(self.create_components())
    }

    /// Attaches this view's components to the given collection.
    fn attach<'a>(&self, components: &mut impl Extend<CreateComponent<'a>>) {
        components.extend(self.create_components());
    }
}

impl<'a, T: ResponseComponentView> ViewProvider<'a> for T {
    fn create(&self) -> Vec<CreateComponent<'a>> {
        self.create_components()
    }
}

/// Trait for views with stored context and state.
#[async_trait::async_trait]
pub trait StatefulView<'a, D = ()>: ResponseComponentView + Send + Sync
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

    /// Edits the stored reply with updated components.
    async fn edit(&self) -> Result<(), Error> {
        if let Some(handle) = &self.view_context().reply_handle {
            handle
                .edit(*self.view_context().poise_ctx, self.create_reply())
                .await?;
        }
        Ok(())
    }
}

/// Trait for views that handle component interactions.
#[async_trait::async_trait]
pub trait InteractableComponentView<'a, T, D = ()>: StatefulView<'a, D>
where
    T: Action,
    D: Send + Sync + 'static,
{
    /// Handles an interaction and returns the action if recognized.
    async fn handle(&mut self, interaction: &ComponentInteraction) -> Option<T>;

    /// Waits for a single interaction and handles it.
    async fn listen_once(&mut self) -> Option<(T, ComponentInteraction)> {
        let ctx = self.view_context();
        let mut collector = ComponentInteractionCollector::new(ctx.poise_ctx.serenity_context())
            .author_id(ctx.poise_ctx.author().id)
            .timeout(ctx.timeout)
            .filter(move |i| T::ALL.contains(&i.data.custom_id.as_str()));

        // Filter by message ID if we have a reply handle
        if let Some(msg_id) = ctx.message_id().await {
            collector = collector.message_id(msg_id);
        }

        let interaction = collector.next().await?;

        interaction
            .create_response(
                self.view_context().poise_ctx.http(),
                CreateInteractionResponse::Acknowledge,
            )
            .await
            .ok();

        self.handle(&interaction)
            .await
            .map(|action| (action, interaction))
    }
}

/// Trait for action enums used in interactive views.
pub trait Action: FromStr + Send {
    /// All possible action strings.
    const ALL: &'static [&'static str];

    /// Returns the custom_id for this action.
    fn custom_id(&self) -> &'static str;

    /// Returns a human-readable label for this action.
    fn label(&self) -> &'static str;
}

/// Generates an enum that implements the `Action` trait for use in interactive Discord views.
///
/// # Syntax
///
/// ```rust
/// # use pwr_bot::custom_id_enum;
/// custom_id_enum! {
///     EnumName {
///         Variant1,
///         Variant2 = "Custom Label",
///         Variant3,
///     }
/// }
/// ```
///
/// # Generated Code
///
/// - Derives: `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`
/// - Implements `Action` trait with:
///   - `custom_id()`: Returns `"EnumName_Variant"`
///   - `label()`: Returns custom label or stringified variant name
/// - Implements `FromStr` for parsing custom IDs back to enum variants
///
/// # Example
///
/// ```rust
/// use pwr_bot::custom_id_enum;
/// use pwr_bot::bot::views::Action;
///
/// custom_id_enum! {
///     SettingsAction {
///         Enable = "Enable Feature",
///         Disable = "Disable Feature",
///         Configure,  // Label will be "Configure"
///     }
/// }
///
/// let action = SettingsAction::Enable;
/// assert_eq!(action.custom_id(), "SettingsAction_Enable");
/// assert_eq!(action.label(), "Enable Feature");
/// ```
#[macro_export]
macro_rules! custom_id_enum {
    (
        $(#[$meta:meta])*
        $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident $(= $label:literal)?
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )*
        }

        impl $crate::bot::views::Action for $name {
            #[doc = "All possible custom_id strings for this action enum."]
            const ALL: &'static [&'static str] = &[
                $(concat!(stringify!($name), "_", stringify!($variant)),)*
            ];

            #[doc = "Returns the Discord custom_id for this action."]
            #[doc = ""]
            #[doc = "Format: `EnumName_Variant`"]
            fn custom_id(&self) -> &'static str {
                match self {
                    $(Self::$variant => concat!(stringify!($name), "_", stringify!($variant)),)*
                }
            }

            #[doc = "Returns the human-readable label for this action."]
            #[doc = ""]
            #[doc = "Uses custom label if provided, otherwise uses the variant name."]
            fn label(&self) -> &'static str {
                match self {
                    $(Self::$variant => custom_id_enum!(@label $variant $(, $label)?),)*
                }
            }
        }

        impl std::str::FromStr for $name {
            type Err = ();

            #[doc = "Parses a custom_id string into an action variant."]
            #[doc = ""]
            #[doc = "Returns `Err(())` if the string doesn't match any variant."]
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $(concat!(stringify!($name), "_", stringify!($variant)) => Ok(Self::$variant),)*
                    _ => Err(()),
                }
            }
        }
    };

    (@label $variant:ident, $label:literal) => { $label };
    (@label $variant:ident) => { stringify!($variant) };
}
