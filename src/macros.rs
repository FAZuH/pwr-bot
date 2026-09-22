/// Generates boilerplate for a Handler implementation.
///
/// This macro creates a handler struct with constructor and Handler
/// fields: the run method reads context from the coordinator.
///
/// # Syntax
///
/// ```rust,ignore
/// handler! {
///     pub struct MyHandler {
///         field1: Type1,
///         field2: Type2,
///     }
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use pwr_bot::handler;
/// use pwr_bot::bot::navigation::Navigation;
///
/// handler! {
///     pub struct MySettingsHandler {}
/// }
///
/// // Then implement the run method:
/// #[async_trait::async_trait]
/// impl CommandHandler for MySettingsHandler {
///     async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
///         let ctx = *coordinator.context();
///         ctx.defer().await?;
///
///         // Handler logic here
///
///         Ok(Navigation::Exit)
///     }
/// }
/// ```
#[macro_export]
macro_rules! handler {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field:ident : $field_type:ty
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $(
                $(#[$field_meta:meta])*
                pub $field: $field_type,
            )*
        }

        impl $name {
            /// Creates a new handler instance.
            pub fn new(
                $($field: $field_type),*
            ) -> Self {
                Self {
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

        impl $crate::bot::view::Action for $name {
            fn label(&self) -> &'static str {
                match self {
                    $(
                        action_enum!(@match_pattern inner, Self::$variant $(, tuple: $($tuple_field)*)? $(, struct: $($struct_field)*)?) => {
                            action_enum!(@label $variant $(, literal: $label)? $(, tuple: $($tuple_field)*)?)
                        },
                    )*
                }
            }
        }
    };

    // Pattern helpers - now take $inner:ident to bind the captured value
    (@match_pattern $inner:ident, $path:path) => { $path };
    (@match_pattern $inner:ident, $path:path, tuple: $($field:tt)+) => { $path(..) };
    (@match_pattern $inner:ident, $path:path, struct: $($field:tt)+) => { $path { .. } };

    // Label helpers
    (@label $variant:ident) => { stringify!($variant) };
    (@label $variant:ident, tuple: $($field:tt)+) => { stringify!($variant) };
    (@label $variant:ident, literal: $label:literal) => { $label };
    (@label $variant:ident, literal: $label:literal, tuple: $($field:tt)+) => { $label };
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
///         #[delegate_label]
///         ExtraVariant3(SomeActionType),
///     }
/// }
/// ```
///
/// `#[delegate_label]` on a tuple variant calls `.label()` on the first field,
/// useful when the inner type implements `Action`.
#[macro_export]
macro_rules! action_extends {
    (
        $(#[$meta:meta])*
        $name:ident extends $base:ty {
            $(
                $(#[doc = $doc:literal])*
                $(#[label = $label:literal])?
                $(#[delegate_label])?
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

        impl $crate::bot::view::Action for $name {
            fn label(&self) -> &'static str {
                match self {
                    Self::Base(base) => base.label(),
                    $(
                        action_extends!(@match_pattern inner, Self::$variant $(, tuple: $($tuple_field)*)? $(, struct: $($struct_field)*)?) => {
                            action_extends!(@label inner, $variant $(, literal: $label)? $(, tuple: $($tuple_field)*)?)
                        },
                    )*
                }
            }
        }
    };

    // Pattern helpers - now take $inner:ident to bind the captured value
    (@match_pattern $inner:ident, $path:path) => { $path };
    (@match_pattern $inner:ident, $path:path, tuple: $($field:tt)+) => { $path($inner, ..) };
    (@match_pattern $inner:ident, $path:path, struct: $($field:tt)+) => { $path { .. } };

    // Label helpers - using tt matchers to pass through the attribute as a token
    (@label $inner:ident, $variant:ident) => { stringify!($variant) };
    (@label $inner:ident, $variant:ident, literal: $label:literal) => { $label };
    (@label $inner:ident, $variant:ident, literal: $label:literal, tuple: $($field:tt)+) => { $label };
    (@label $inner:ident, $variant:ident, tuple: $($field:tt)+) => { $inner.label() };
}
