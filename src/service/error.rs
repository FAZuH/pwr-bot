use crate::database::error::DatabaseError;
use crate::feed::error::FeedError;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServiceError {
    #[error("Unexpected result: {message}")]
    UnexpectedResult { message: String },

    #[error("FeedError: {0}")]
    FeedError(#[from] FeedError),

    #[error("DatabaseError: {0}")]
    DatabaseError(#[from] DatabaseError),
}
