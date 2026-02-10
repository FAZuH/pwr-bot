use std::time::Duration;

use async_trait::async_trait;
use poise::CreateReply;
use serenity::all::CreateComponent;
use serenity::all::CreateMessage;
use serenity::all::MessageFlags;

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

#[async_trait]
pub trait InteractableComponentView<T>: for<'a> AttachableView<'a> {
    async fn listen(&mut self, timeout: Duration) -> T;
}

#[macro_export]
macro_rules! custom_id_enum {
    ($name:ident { $($variant:ident),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $($variant,)*
        }

        impl $name {
            pub const fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant),)*
                }
            }

            pub const ALL: &'static [&'static str] = &[
                $(stringify!($variant),)*
            ];
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
