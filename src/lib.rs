//! pwr-bot — A modular Discord bot with a dynamic plugin system.
//!
//! The bot loads plugins as `.so` shared libraries from `PLUGIN_DIR` at startup.
//! Plugins communicate with the host via a stable C ABI defined in
//! [`pwr_bot_sdk`]. The core is fully decoupled — it has no compile-time
//! knowledge of any specific plugin.
//!
//! ## Architecture
//!
//! - **Plugin SDK** ([`pwr_bot_sdk`]) — types and macros for writing plugins
//! - **Plugin Registry** ([`bot::plugin::registry::PluginRegistry`]) — dynamic discovery of commands, panels, events, and tasks
//! - **Host Context** ([`bot::host_ctx::HostCtx`]) — abstraction over Discord I/O (poise for built-in, FFI for plugins)
//! - **Event Bus** ([`event::event_bus::EventBus`]) — typed + named pub/sub for cross-component communication
//!
//! ## Features
//! - Dynamic plugin loading from `PLUGIN_DIR` at startup
//! - Plugin registry for commands, settings panels, event handlers, and tasks
//! - Voice channel activity scanning published as `voice_state` events
//! - Server configuration management with per-plugin enable/disable

pub mod bot;
pub mod config;
pub mod entity;
pub mod error;
pub mod event;
pub mod logging;
pub mod macros;
pub mod repo;
pub mod service;
pub mod task;
pub mod update;
