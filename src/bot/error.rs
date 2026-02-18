//! Bot-specific error types.

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BotError {
    #[error("Invalid argument for {parameter}: {reason}")]
    InvalidCommandArgument { parameter: String, reason: String },

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("You have to be in a server to use this command")]
    GuildOnlyCommand,

    #[error("{0}")]
    UserNotInGuild(String),
}
