//! Bot-specific error types.

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BotError {
    #[error("Invalid argument for {parameter}: {reason}")]
    InvalidCommandArgument { parameter: String, reason: String },

    /// A plugin this guild switched off, so the host refuses to serve it here.
    /// The process keeps running for every other guild, so the refusal names
    /// the plugin and the guild rather than the plugin being gone.
    #[error("Plugin `{plugin}` is disabled in this server.")]
    PluginDisabledInGuild { plugin: String },

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("You have to be in a server to use this command")]
    GuildOnlyCommand,

    #[error("User not in server")]
    UserNotInGuild(String),
}
