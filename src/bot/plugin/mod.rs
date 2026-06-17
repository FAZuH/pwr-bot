//! Plugin system for loading dynamic `.so` plugins.
//!
//! Plugins communicate via a stable C ABI defined in the `pwr_bot_sdk` crate.
//! The host loads plugins at startup, registers their slash commands with Discord,
//! and delegates matching invocations back to the plugin via FFI.

pub mod ffi_host_ctx;
pub mod host_registry;
pub mod invocation;
pub mod loader;
pub mod registry;
pub mod view_registry;
