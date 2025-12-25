use crate::database::error::DatabaseError;
use crate::feed::error::SeriesError;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AppError {
    #[error("Assertion error: {msg}")]
    AssertionError { msg: String },

    #[error("Missing config with key \"{key}\"")]
    MissingConfig { key: String },
}

pub enum AppErrorKind {
    AppError(AppError),
    DatabaseError(DatabaseError),
    SeriesError(SeriesError),
}
