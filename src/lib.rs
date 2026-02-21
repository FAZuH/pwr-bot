//! pwr-bot - A Discord bot with feed subscriptions and voice channel tracking.
//!
//! This crate provides a Discord bot implementation with features including:
//! - Feed subscriptions (MangaDex, AniList, Comick)
//! - Voice channel activity tracking and leaderboards
//! - Server configuration management

pub mod bot;
pub mod config;
pub mod error;
pub mod event;
pub mod feed;
pub mod logging;
pub mod macros;
pub mod entity;
pub mod repository;
pub mod service;
pub mod subscriber;
pub mod task;

/// Trait for types that hold internal data of type `T`.
///
/// Provides immutable and mutable access to the internal data.
pub trait WithData<T> {
    /// Returns an immutable reference to the internal data.
    fn data(&self) -> &T;

    /// Returns a mutable reference to the internal data.
    fn data_mut(&mut self) -> &mut T;
}
