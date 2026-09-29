//! pwr-bot host process for Discord plugin services.
//!
//! Plugin-owned domains live in separate plugin crates. This crate owns host
//! routing, the Settings capability, and core plugin state.

pub mod bot;
pub mod config;
pub mod entity;
pub mod error;
pub mod event;
pub mod logging;
pub mod macros;
pub mod plugin;
pub mod repo;
pub mod service;
pub mod update;

#[cfg(test)]
pub(crate) mod test_helpers;
