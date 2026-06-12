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

/// Generates boilerplate for a Handler implementation.
///
/// This macro creates a handler struct with a `host_ctx` field, constructor,
/// and Handler trait implementation with the run method signature.
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
            pub host_ctx: std::sync::Arc<$crate::bot::host_ctx::PoiseHostCtx>,
            $(
                $(#[$field_meta:meta])*
                pub $field: $field_type,
            )*
        }

        impl $name {
            /// Creates a new handler instance.
            pub fn new(
                host_ctx: std::sync::Arc<$crate::bot::host_ctx::PoiseHostCtx>,
                $($field: $field_type),*
            ) -> Self {
                Self {
                    host_ctx,
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

        impl $name {
            #[doc = "Create from base action"]
            pub fn from_base(base: $base) -> Self {
                Self::Base(base)
            }
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
