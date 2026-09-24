//! pwr-bot host process for Discord plugin and voice services.
//!
//! Feed subscriptions and their settings live in the `feed` plugin. This
//! crate owns host routing, shared voice services, and core plugin state.

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
pub mod subscriber;
pub mod task;
pub mod update;

#[cfg(test)]
pub(crate) mod test_helpers;
