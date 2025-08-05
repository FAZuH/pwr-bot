pub mod latest_results_table;
pub mod subscribers_table;

use async_trait::async_trait;
use sqlx::Error as DbError;
use sqlx::SqlitePool;

pub struct BaseTable {
    pub pool: SqlitePool,
}

impl BaseTable {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
pub trait Table<T, ID> {
    async fn create_table(&self) -> Result<(), DbError>;
    async fn drop_table(&self) -> Result<(), DbError>;
    async fn select_all(&self) -> Result<Vec<T>, DbError>;
    async fn delete_all(&self) -> Result<(), DbError>;
    async fn insert(&self, model: &T) -> Result<ID, DbError>;
    async fn select(&self, id: &ID) -> Result<T, DbError>;
    async fn update(&self, model: &T) -> Result<(), DbError>;
    async fn delete(&self, id: &ID) -> Result<(), DbError>;
}
