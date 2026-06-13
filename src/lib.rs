//! pwr-bot - A modular Discord bot with hot-pluggable plugin system.
//!
//! ## Features
//! - Dynamic plugin loading from `PLUGIN_DIR` at startup
//! - Plugin registry for commands, settings panels, event handlers, and tasks
//! - Voice channel activity scanning published as `voice_state` events
//! - Server configuration management

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
