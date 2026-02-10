use std::str::FromStr;
use std::time::Duration;

use futures::Stream;
use futures::StreamExt;
use futures::stream::unfold;
use poise::CreateReply;
use serenity::all::ComponentInteraction;
use serenity::all::ComponentInteractionCollector;
use serenity::all::CreateComponent;
use serenity::all::CreateInteractionResponse;
use serenity::all::CreateMessage;
use serenity::all::MessageFlags;

use crate::bot::commands::Context;

pub mod pagination;

pub trait ViewProvider<'a, T = CreateComponent<'a>> {
    fn create(&self) -> Vec<T>;
}

pub trait ResponseComponentView: for<'a> ViewProvider<'a> {
    fn create_reply(&self) -> CreateReply<'static> {
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(self.create())
    }

    fn create_message(&self) -> CreateMessage<'static> {
        CreateMessage::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(self.create())
    }
}

pub trait AttachableView<'a, T = CreateComponent<'a>>: ViewProvider<'a, T> {
    fn attach(&self, components: &mut impl Extend<T>) {
        components.extend(self.create());
    }
}

impl<'a, T> AttachableView<'a> for T where T: ViewProvider<'a> {}

#[async_trait::async_trait]
pub trait InteractableComponentView<T>: for<'a> AttachableView<'a>
where
    T: Action,
{
    async fn listen_once<'a>(
        &mut self,
        ctx: &'a Context<'a>,
        timeout: Duration,
    ) -> Option<(T, ComponentInteraction)> {
        let collector = ComponentInteractionCollector::new(ctx.serenity_context())
            .author_id(ctx.author().id)
            .filter(move |i| T::ALL.contains(&i.data.custom_id.as_str()))
            .timeout(timeout);

        let interaction = collector.next().await?;

        interaction
            .create_response(ctx.http(), CreateInteractionResponse::Acknowledge)
            .await
            .ok();

        self.handle(&interaction)
            .await
            .map(|action| (action, interaction))
    }

    /// Returns a stream that processes each interaction through `handle` before yielding
    fn stream<'a>(
        &'a mut self,
        ctx: &'a Context<'a>,
        timeout: Duration,
    ) -> impl Stream<Item = (T, ComponentInteraction)> + Send + 'a
    where
        Self: Send,
        <T as std::str::FromStr>::Err: Send,
    {
        let collector = ComponentInteractionCollector::new(ctx.serenity_context())
            .author_id(ctx.author().id)
            .filter(move |i| T::ALL.contains(&i.data.custom_id.as_str()))
            .timeout(timeout)
            .stream();

        unfold(
            (self, ctx, collector),
            |(view, ctx, mut collector)| async move {
                while let Some(interaction) = collector.next().await {
                    interaction
                        .create_response(ctx.http(), CreateInteractionResponse::Acknowledge)
                        .await
                        .ok();

                    if let Some(action) = view.handle(&interaction).await {
                        return Some(((action, interaction), (view, ctx, collector)));
                    }
                    // If `[Self::Handle]` returns `[Option::None]`, continue to next iteration
                }
                None
            },
        )
    }

    async fn handle(&mut self, interaction: &ComponentInteraction) -> Option<T>;
}

pub trait Action: FromStr + Send {
    const ALL: &'static [&'static str];
    fn as_str(&self) -> &'static str;
}

#[macro_export]
macro_rules! custom_id_enum {
    ($name:ident { $($variant:ident),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $($variant,)*
        }

        impl $crate::bot::views::Action for $name {
            const ALL: &'static [&'static str] = &[
                $(stringify!($variant),)*
            ];

            fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant),)*
                }
            }
        }

        impl std::str::FromStr for $name {
            type Err = ();

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $(stringify!($variant) => Ok(Self::$variant),)*
                    _ => Err(()),
                }
            }
        }
    };
}
