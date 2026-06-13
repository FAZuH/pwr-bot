use async_trait::async_trait;

use crate::entity::*;
use crate::repo::error::DatabaseError;

/// Trait for basic table maintenance operations.
#[async_trait]
pub trait TableBase: Send + Sync {
    async fn create_table(&self) -> Result<(), DatabaseError>;
    async fn drop_table(&self) -> Result<(), DatabaseError>;
    async fn delete_all(&self) -> Result<(), DatabaseError>;
}

/// Generic trait for standard CRUD (Create, Read, Update, Delete) operations.
#[async_trait]
pub trait CrudTable<T, ID>: TableBase {
    async fn select_all(&self) -> Result<Vec<T>, DatabaseError>;
    async fn insert(&self, model: &T) -> Result<ID, DatabaseError>;
    async fn select(&self, id: &ID) -> Result<Option<T>, DatabaseError>;
    async fn update(&self, model: &T) -> Result<(), DatabaseError>;
    async fn delete(&self, id: &ID) -> Result<(), DatabaseError>;
    async fn replace(&self, model: &T) -> Result<ID, DatabaseError>;
}

/// Operations for the `server_settings` table.
#[async_trait]
pub trait ServerSettingsRepository: CrudTable<ServerSettingsEntity, u64> + Send + Sync {}

/// Operations for internal bot metadata.
#[async_trait]
pub trait BotMetaRepository: CrudTable<BotMetaEntity, String> + Send + Sync {
    async fn table_exists(&self) -> bool;
}

/// Factory trait providing access to individual repository handles.
pub trait Repos: Send + Sync {
    fn server_settings(&self) -> Box<dyn ServerSettingsRepository + Send + Sync>;
    fn bot_meta(&self) -> Box<dyn BotMetaRepository + Send + Sync>;
}
