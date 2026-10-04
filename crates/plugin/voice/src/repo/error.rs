use diesel::result::Error as DieselError;

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("database error: {0}")]
    Backend(#[from] DieselError),
    #[error("database pool error: {0}")]
    Pool(String),
    #[error("database task error: {0}")]
    Task(String),
    #[error("invalid database query: {0}")]
    InvalidQuery(String),
}

impl From<tokio::task::JoinError> for DatabaseError {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::Task(error.to_string())
    }
}

impl From<diesel_async::pooled_connection::deadpool::PoolError> for DatabaseError {
    fn from(error: diesel_async::pooled_connection::deadpool::PoolError) -> Self {
        Self::Pool(error.to_string())
    }
}
