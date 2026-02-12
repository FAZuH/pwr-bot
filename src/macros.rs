/// Generates a struct that implements `StatefulView` for Discord UI components.
///
/// This macro wraps a struct definition and automatically adds a `ctx` field
/// of type `ViewContext<'a, D>` and implements the `StatefulView` trait.
///
/// # Syntax
///
/// For views with `()` as the data type (most common case):
/// ```rust,ignore
/// stateful_view! {
///     timeout = Duration::from_secs(120),
///     pub struct MyView<'a> {
///         pub public_field: Type1,
///         private_field: Type2,
///     }
/// }
/// ```
///
/// For views with a custom data type:
/// ```rust,ignore
/// stateful_view! {
///     DataType = MyData,
///     timeout = Duration::from_secs(120),
///     pub struct MyView<'a> {
///         pub public_field: Type1,
///         private_field: Type2,
///     }
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use std::time::Duration;
/// use crate::bot::views::StatefulView;
///
/// stateful_view! {
///     timeout = Duration::from_secs(120),
///     pub struct SettingsFeedView<'a> {
///         pub settings: &'a mut ServerSettings,
///     }
/// }
///
/// // The macro generates:
/// // - A struct with an added `ctx: ViewContext<'a, ()>` field
/// // - Implementation of `StatefulView<'a, ()>` with `view_context()` and `view_context_mut()`
/// // - A `new()` constructor that initializes the context
/// ```
#[macro_export]
macro_rules! stateful_view {
    // Variant 1: Default data type ()
    (
        timeout = $timeout:expr,
        $(#[$meta:meta])*
        $vis:vis struct $name:ident<$lt:lifetime> {
            $($visf:vis $field:ident : $field_type:ty),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name<$lt> {
            /// View context for managing the view lifecycle.
            ///
            /// This context stores the poise context, timeout duration, reply handle,
            /// and optional data for the view. It provides methods for sending and
            /// editing messages.
            ctx: $crate::bot::views::ViewContext<$lt, ()>,
            $(
                #[doc = concat!("Field `", stringify!($field), "`.")]
                $visf $field: $field_type,
            )*
        }

        impl<$lt> $name<$lt> {
            /// Creates a view context with the given poise context and configured timeout.
            ///
            /// This is a helper method used internally to create the view context.
            /// The timeout determines how long the view will wait for interactions.
            pub fn create_context(
                ctx: &$lt $crate::bot::commands::Context<$lt>,
            ) -> $crate::bot::views::ViewContext<$lt, ()> {
                $crate::bot::views::ViewContext::new(ctx, $timeout)
            }
        }

        #[async_trait::async_trait]
        impl<$lt> $crate::bot::views::StatefulView<$lt, ()> for $name<$lt> {
            fn view_context(&self) -> &$crate::bot::views::ViewContext<$lt, ()> {
                &self.ctx
            }

            fn view_context_mut(&mut self) -> &mut $crate::bot::views::ViewContext<$lt, ()> {
                &mut self.ctx
            }
        }
    };

    // Variant 2: Custom data type
    (
        DataType = $data_type:ty,
        timeout = $timeout:expr,
        $(#[$meta:meta])*
        $vis:vis struct $name:ident<$lt:lifetime> {
            $($field:ident : $field_type:ty),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name<$lt> {
            /// View context for managing the view lifecycle.
            ///
            /// This context stores the poise context, timeout duration, reply handle,
            /// and custom data for the view. It provides methods for sending and
            /// editing messages.
            ctx: $crate::bot::views::ViewContext<$lt, $data_type>,
            $(
                #[doc = concat!("Field `", stringify!($field), "`.")]
                $vis $field: $field_type,
            )*
        }

        impl<$lt> $name<$lt> {
            /// Creates a new view instance with the given context, custom data, and fields.
            ///
            /// # Arguments
            ///
            /// * `ctx` - The poise context for Discord interactions
            /// * `data` - Custom data to store in the view context
            /// * `$($field)` - The struct fields defined in the macro invocation
            ///
            /// # Example
            ///
            /// ```rust,ignore
            /// let view = MyView::with_data(&ctx, my_data, field1_value, field2_value);
            /// ```
            pub fn with_data(
                ctx: &$lt $crate::bot::commands::Context<$lt>,
                data: $data_type,
                $($field: $field_type),*
            ) -> Self {
                Self {
                    ctx: Self::create_context(ctx, data),
                    $($field,)*
                }
            }

            /// Creates a view context with the given poise context, data, and configured timeout.
            ///
            /// This is a helper method used internally to create the view context
            /// with custom data. The timeout determines how long the view will wait
            /// for interactions.
            pub fn create_context(
                ctx: &$lt $crate::bot::commands::Context<$lt>,
                data: $data_type,
            ) -> $crate::bot::views::ViewContext<$lt, $data_type> {
                $crate::bot::views::ViewContext::with_data(ctx, $timeout, data)
            }
        }

        #[async_trait::async_trait]
        impl<$lt> $crate::bot::views::StatefulView<$lt, $data_type> for $name<$lt> {
            fn view_context(&self) -> &$crate::bot::views::ViewContext<$lt, $data_type> {
                &self.ctx
            }

            fn view_context_mut(&mut self) -> &mut $crate::bot::views::ViewContext<$lt, $data_type> {
                &mut self.ctx
            }
        }
    };
}

/// Generates a struct with an internal `data` field and implements `WithData<T>`.
///
/// # Syntax
///
/// ```rust,ignore
/// with_data! {
///     DataType,
///     pub struct MyStruct<'a> {
///         field1: Type1,
///         field2: Type2,
///     }
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// struct MyData {
///     count: i32,
/// }
///
/// with_data! {
///     MyData,
///     pub struct Counter<'a> {
///         name: &'a str,
///     }
/// }
///
/// let mut counter = Counter {
///     data: MyData { count: 0 },
///     name: "test",
/// };
///
/// counter.data_mut().count += 1;
/// assert_eq!(counter.data().count, 1);
/// ```
#[macro_export]
macro_rules! with_data {
    (
        $data_type:ty,
        $(#[$meta:meta])*
        $vis:vis struct $name:ident<$lt:lifetime> {
            $($field:ident : $field_type:ty),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name<$lt> {
            /// Internal data storage.
            pub data: $data_type,
            $(
                #[doc = concat!("Field `", stringify!($field), "`.")]
                pub $field: $field_type,
            )*
        }

        impl<$lt> $crate::WithData<$data_type> for $name<$lt> {
            fn data(&self) -> &$data_type {
                &self.data
            }
            fn data_mut(&mut self) -> &mut $data_type {
                &mut self.data
            }
        }
    };
}

/// Generates boilerplate for a Controller implementation.
///
/// This macro creates a controller struct with context field, constructor,
/// and Controller trait implementation with the run method signature.
///
/// # Syntax
///
/// ```rust,ignore
/// controller! {
///     pub struct MyController<'a> {
///         field1: Type1,
///         field2: Type2,
///     }
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use pwr_bot::controller;
/// use pwr_bot::bot::navigation::NavigationResult;
///
/// controller! {
///     pub struct MySettingsController<'a> {}
/// }
///
/// // Then implement the run method:
/// #[async_trait::async_trait]
/// impl<S: Send + Sync + 'static> Controller<S> for MySettingsController<'_> {
///     async fn run(&mut self, coordinator: &mut Coordinator<'_, S>) -> Result<NavigationResult, Error> {
///         let ctx = *coordinator.context();
///         ctx.defer().await?;
///         
///         // Controller logic here
///         
///         Ok(NavigationResult::Exit)
///     }
/// }
/// ```
#[macro_export]
macro_rules! controller {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident<$lt:lifetime> {
            $($field:ident : $field_type:ty),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name<$lt> {
            #[allow(dead_code)]
            ctx: &$lt $crate::bot::commands::Context<$lt>,
            $(
                #[doc = concat!("Field `", stringify!($field), "`.")]
                pub $field: $field_type,
            )*
        }

        impl<$lt> $name<$lt> {
            /// Creates a new controller instance.
            pub fn new(
                ctx: &$lt $crate::bot::commands::Context<$lt>,
                $($field: $field_type),*
            ) -> Self {
                Self {
                    ctx,
                    $($field,)*
                }
            }
        }
    };
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
