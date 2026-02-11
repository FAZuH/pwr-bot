//! pwr-bot - A Discord bot with feed subscriptions and voice channel tracking.
//!
//! This crate provides a Discord bot implementation with features including:
//! - Feed subscriptions (MangaDex, AniList, Comick)
//! - Voice channel activity tracking and leaderboards
//! - Server configuration management

pub mod bot;
pub mod config;
pub mod database;
pub mod error;
pub mod event;
pub mod feed;
pub mod logging;
pub mod service;
pub mod subscriber;
pub mod task;
