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
            $(
                $(#[$field_meta:meta])*
                $field:ident : $field_type:ty
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name<$lt> {
            #[allow(dead_code)]
            ctx: &$lt $crate::bot::commands::Context<$lt>,
            $(
                $(#[$field_meta])*
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
/// ```rust,ignore
/// # use pwr_bot::action_enum;
/// action_enum! {
///     EnumName {
///         #[label = "Custom Label"]
///         Variant1,
///         Variant2,
///         #[label = "Set Value"]
///         Variant3(u32),
///         Variant4 { field: Type },
///     }
/// }
/// ```
///
/// # Generated Code
///
/// - Derives: `Debug`, `Clone`, `PartialEq`, `Eq`
/// - Implements `Action` trait with `label()` method
///
/// # Example
///
/// ```rust,ignore
/// use pwr_bot::action_enum;
/// use pwr_bot::bot::views::Action;
///
/// action_enum! {
///     SettingsAction {
///         #[label = "Enable Feature"]
///         Enable,
///         Disable,
///         #[label = "Set Value"]
///         SetValue(u32),
///         Update { name: String },
///     }
/// }
///
/// let action = SettingsAction::Enable;
/// assert_eq!(action.label(), "Enable Feature");
///
/// let action = SettingsAction::SetValue(42);
/// assert_eq!(action.label(), "Set Value");
///
/// let action = SettingsAction::Disable;
/// assert_eq!(action.label(), "Disable");
/// ```
#[macro_export]
macro_rules! action_enum {
    (
        $(#[$meta:meta])*
        $name:ident {
            $(
                $(#[doc = $doc:literal])*
                $(#[label = $label:literal])?
                $variant:ident
                $( ( $($tuple_field:ty),* $(,)? ) )?
                $( { $($struct_field:ident : $struct_type:ty),* $(,)? } )?
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum $name {
            $(
                $(#[doc = $doc])*
                $variant $( ( $($tuple_field),* ) )? $( { $($struct_field : $struct_type),* } )?,
            )*
        }

        impl $crate::bot::views::Action for $name {
            #[doc = "Returns the human-readable label for this action."]
            fn label(&self) -> &'static str {
                match self {
                    $(
                        action_enum!(@match_pattern Self::$variant $(, tuple: $($tuple_field)*)? $(, struct: $($struct_field)*)?) => {
                            action_enum!(@label $variant $(, $label)?)
                        }
                    )*
                }
            }
        }
    };

    (@label $variant:ident, $label:literal) => { $label };
    (@label $variant:ident) => { stringify!($variant) };

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
/// ```rust,ignore
/// action_extends! {
///     NewEnumName extends BaseEnumName {
///         #[label = "Custom Label"]
///         ExtraVariant1,
///         ExtraVariant2,
///         #[label = "Filter By"]
///         ExtraVariant3(String),
///         ExtraVariant4 { field: Type },
///     }
/// }
/// ```
///
/// # Generated Code
///
/// - Derives: `Debug`, `Clone`, `PartialEq`, `Eq`
/// - Implements `Action` trait with `label()` delegating to base or own variants
///
/// # Example
///
/// ```rust,ignore
/// use pwr_bot::{action_enum, action_extends};
/// use pwr_bot::bot::views::Action;
///
/// action_enum! {
///     PaginationAction {
///         Prev,
///         Next,
///     }
/// }
///
/// action_extends! {
///     FeedListAction extends PaginationAction {
///         #[label = "Close"]
///         Exit,
///         Refresh,
///         #[label = "Filter By"]
///         Filter(String),
///     }
/// }
///
/// let action = FeedListAction::Exit;
/// assert_eq!(action.label(), "Close");
///
/// let action = FeedListAction::Base(PaginationAction::Prev);
/// assert_eq!(action.label(), "Prev");
/// ```
#[macro_export]
macro_rules! action_extends {
    (
        $(#[$meta:meta])*
        $name:ident extends $base:ty {
            $(
                $(#[doc = $doc:literal])*
                $(#[label = $label:literal])?
                $variant:ident
                $( ( $($tuple_field:ty),* $(,)? ) )?
                $( { $($struct_field:ident : $struct_type:ty),* $(,)? } )?
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum $name {
            #[doc = "Variants from the extended action"]
            Base($base),
            $(
                $(#[doc = $doc])*
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
            fn label(&self) -> &'static str {
                match self {
                    Self::Base(base) => base.label(),
                    $(
                        action_extends!(@match_pattern Self::$variant $(, tuple: $($tuple_field)*)? $(, struct: $($struct_field)*)?) => {
                            action_extends!(@label $variant $(, $label)?)
                        }
                    )*
                }
            }
        }
    };

    (@label $variant:ident, $label:literal) => { $label };
    (@label $variant:ident) => { stringify!($variant) };

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

#[macro_export]
macro_rules! view_core {
    (
        timeout = $timeout:expr,
        $(#[$meta:meta])*
        $vis:vis struct $name:ident<$lt:lifetime, $action:ty> {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $field_type:ty
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name<$lt> {
            #[allow(dead_code)]
            core: $crate::bot::views::ViewCore<$lt, $action>,
            $(
                $(#[$field_meta])*
                $field_vis $field: $field_type,
            )*
        }

        impl<$lt> $crate::bot::views::View<$lt, $action> for $name<$lt> {
            fn core(&self) -> &$crate::bot::views::ViewCore<$lt, $action> {
                &self.core
            }
            fn core_mut(&mut self) -> &mut $crate::bot::views::ViewCore<$lt, $action> {
                &mut self.core
            }
            fn create_core(poise_ctx: &$lt $crate::bot::commands::Context<$lt>) -> $crate::bot::views::ViewCore<$lt, $action> {
                $crate::bot::views::ViewCore::new(poise_ctx, $timeout)
            }
        }
    };
}
