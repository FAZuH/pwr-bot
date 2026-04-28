//! TEA-style update module for pure business logic.
//!
//! Separates state mutations and side-effect commands from UI rendering,
//! making the core logic fully unit-testable without mocking Discord.

/// The Elm Architecture update trait.
///
/// Receives a message and the current model, mutates the model in-place,
/// and returns a command describing any side effects the caller should perform.
pub trait Update {
    type Model;
    type Msg;
    type Cmd;

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Self::Cmd;
}

pub mod voice_leaderboard;
