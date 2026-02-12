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
///         Variant3(Type1, Type2),
///         Variant4 { field: Type } = "Label",
///     }
/// }
/// ```
///
/// # Generated Code
///
/// - Derives: `Debug`, `Clone`, `PartialEq`, `Eq` (Copy only if all fields are Copy)
/// - Implements `Action` trait with:
///   - `custom_id()`: Returns `"EnumName_Variant"`
///   - `label()`: Returns custom label or stringified variant name
/// - Implements `FromStr` for parsing custom IDs back to enum variants (unit variants only)
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
///         Configure,
///         SetValue(u32),
///         Update { name: String, value: i32 },
///     }
/// }
///
/// let action = SettingsAction::Enable;
/// assert_eq!(action.custom_id(), "SettingsAction_Enable");
/// assert_eq!(action.label(), "Enable Feature");
///
/// let action = SettingsAction::SetValue(42);
/// assert_eq!(action.custom_id(), "SettingsAction_SetValue");
/// ```
#[macro_export]
macro_rules! custom_id_enum {
    (
        $(#[$meta:meta])*
        $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident
                $( ( $($tuple_field:ty),* $(,)? ) )?
                $( { $($struct_field:ident : $struct_type:ty),* $(,)? } )?
                $(= $label:literal)?
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum $name {
            $(
                $(#[$variant_meta])*
                $variant $( ( $($tuple_field),* ) )? $( { $($struct_field : $struct_type),* } )?,
            )*
        }

        impl $crate::bot::views::Action for $name {
            #[doc = "All possible custom_id strings for this action enum."]
            fn all() -> &'static [&'static str] {
                &[$(concat!(stringify!($name), "_", stringify!($variant)),)*]
            }

            #[doc = "Returns the Discord custom_id for this action."]
            #[doc = ""]
            #[doc = "Format: `EnumName_Variant`"]
            fn custom_id(&self) -> &'static str {
                match self {
                    $(
                        custom_id_enum!(@match_pattern Self::$variant $(, tuple: $($tuple_field)*)? $(, struct: $($struct_field)*)?) => {
                            concat!(stringify!($name), "_", stringify!($variant))
                        }
                    )*
                }
            }

            #[doc = "Returns the human-readable label for this action."]
            #[doc = ""]
            #[doc = "Uses custom label if provided, otherwise uses the variant name."]
            fn label(&self) -> &'static str {
                match self {
                    $(
                        custom_id_enum!(@match_pattern Self::$variant $(, tuple: $($tuple_field)*)? $(, struct: $($struct_field)*)?) => {
                            custom_id_enum!(@label $variant $(, $label)?)
                        }
                    )*
                }
            }
        }

        impl std::str::FromStr for $name {
            type Err = ();

            #[doc = "Parses a custom_id string into an action variant."]
            #[doc = ""]
            #[doc = "Only works for unit variants. Returns `Err(())` for variants with data or unrecognized strings."]
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $(
                        concat!(stringify!($name), "_", stringify!($variant)) => {
                            custom_id_enum!(@from_str_result $variant $(, $($tuple_field)*)? $(, $($struct_field)*)?)
                        }
                    )*
                    _ => Err(()),
                }
            }
        }
    };

    (@label $variant:ident, $label:literal) => { $label };
    (@label $variant:ident) => { stringify!($variant) };

    (@from_str_result $variant:ident) => {
        Ok(Self::$variant)
    };

    (@from_str_result $variant:ident, $($field:tt)+) => {
        Err(())
    };
    (@match_pattern $path:path) => {
        $path
    };

    (@match_pattern $path:path, tuple: $($field:tt)+) => {
        $path(..)
    };

    (@match_pattern $path:path, struct: $($field:tt)+) => {
        $path { .. }
    };
}

/// Extends an existing Action enum with additional variants.
///
/// # Syntax
///
/// ```rust
/// custom_id_extends! {
///     NewEnumName extends BaseEnumName {
///         ExtraVariant1,
///         ExtraVariant2 = "Custom Label",
///         ExtraVariant3(Type),
///         ExtraVariant4 { field: Type },
///     }
/// }
/// ```
///
/// # Generated Code
///
/// - Derives: `Debug`, `Clone`, `PartialEq`, `Eq`
/// - Implements `Action` trait with `all()` returning combined IDs from base + new variants
/// - Implements `FromStr` for parsing (base variants parse to their original enum, not this one)
///
/// # Example
///
/// ```rust
/// use pwr_bot::{custom_id_enum, custom_id_extends};
/// use pwr_bot::bot::views::Action;
///
/// custom_id_enum! {
///     PaginationAction {
///         Prev,
///         Next,
///     }
/// }
///
/// custom_id_extends! {
///     FeedListAction extends PaginationAction {
///         Exit = "Close",
///         Refresh,
///         Filter(String),
///     }
/// }
/// ```
#[macro_export]
macro_rules! custom_id_extends {
    (
        $(#[$meta:meta])*
        $name:ident extends $base:ty {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident
                $( ( $($tuple_field:ty),* $(,)? ) )?
                $( { $($struct_field:ident : $struct_type:ty),* $(,)? } )?
                $(= $label:literal)?
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum $name {
            #[doc = "Variants from the extended action"]
            Base($base),
            $(
                $(#[$variant_meta])*
                $variant $( ( $($tuple_field),* ) )? $( { $($struct_field : $struct_type),* } )?,
            )*
        }

        impl $name {
            #[doc = "Create from base action"]
            pub fn from_base(base: $base) -> Self {
                Self::Base(base)
            }
        }

        impl $crate::bot::views::Action for $name {
            fn all() -> &'static [&'static str] {
                use std::sync::LazyLock;
                static ALL: LazyLock<Box<[&'static str]>> = LazyLock::new(|| {
                    <$base as $crate::bot::views::Action>::all()
                        .iter()
                        .copied()
                        .chain([$(concat!(stringify!($name), "_", stringify!($variant))),*])
                        .collect::<Vec<_>>()
                        .into_boxed_slice()
                });
                &ALL
            }

            fn custom_id(&self) -> &'static str {
                match self {
                    Self::Base(base) => base.custom_id(),
                    $(
                        custom_id_extends!(@match_pattern Self::$variant $(, tuple: $($tuple_field)*)? $(, struct: $($struct_field)*)?) => {
                            concat!(stringify!($name), "_", stringify!($variant))
                        }
                    )*
                }
            }

            fn label(&self) -> &'static str {
                match self {
                    Self::Base(base) => base.label(),
                    $(
                        custom_id_extends!(@match_pattern Self::$variant $(, tuple: $($tuple_field)*)? $(, struct: $($struct_field)*)?) => {
                            $crate::custom_id_extends!(@label $variant $(, $label)?)
                        }
                    )*
                }
            }
        }

        impl std::str::FromStr for $name {
            type Err = ();

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                // Try base first
                if let Ok(base) = <$base as std::str::FromStr>::from_str(s) {
                    return Ok(Self::Base(base));
                }
                // Then own variants (unit variants only)
                match s {
                    $(
                        concat!(stringify!($name), "_", stringify!($variant)) => {
                            custom_id_extends!(@from_str_result $variant $(, $($tuple_field)*)? $(, $($struct_field)*)?)
                        }
                    )*
                    _ => Err(()),
                }
            }
        }
    };

    (@label $variant:ident, $label:literal) => { $label };
    (@label $variant:ident) => { stringify!($variant) };

    (@from_str_result $variant:ident) => {
        Ok(Self::$variant)
    };

    (@from_str_result $variant:ident, $($field:tt)+) => {
        Err(())
    };
    (@match_pattern $path:path) => {
        $path
    };

    (@match_pattern $path:path, tuple: $($field:tt)+) => {
        $path(..)
    };

    (@match_pattern $path:path, struct: $($field:tt)+) => {
        $path { .. }
    };
}
