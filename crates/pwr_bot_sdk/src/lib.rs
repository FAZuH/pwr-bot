//! SDK for building pwr-bot plugins.
//!
//! This crate provides the types and macros needed to write a dynamic plugin
//! that can be loaded at runtime by the pwr-bot host. See [`BotPlugin`] for
//! the main entry point.
//!
//! [`BotPlugin`]: plugin::BotPlugin

pub mod abi;
pub mod host;
pub mod macros;
pub mod plugin;

pub use abi::HostCallbacks;
pub use abi::InvokeRequest;
pub use abi::InvokeResponse;
pub use abi::PWR_BOT_PLUGIN_API_VERSION;
pub use abi::PWR_BOT_PLUGIN_ENTRY;
pub use abi::PluginVTable;
pub use host::PluginHost;
pub use plugin::BotPlugin;
pub use plugin::CommandDefinition;
pub use plugin::EventHandlerSpec;
pub use plugin::PluginMetadata;
pub use plugin::ResponsePayload;
pub use plugin::SettingsPanelSpec;
pub use plugin::TaskSpec;
pub use plugin::TestStepSpec;
