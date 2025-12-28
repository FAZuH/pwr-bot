#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BotError {
    #[error("Invalid argument for {parameter}: {reason}")]
    InvalidCommandArgument { parameter: String, reason: String },
}
