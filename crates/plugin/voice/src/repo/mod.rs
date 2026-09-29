pub mod error;
pub mod postgres;
pub mod schema;
pub mod traits;

use crate::repo::error::DatabaseError;
use crate::repo::postgres::PgVoiceSessionsRepo;
use crate::repo::postgres::PgVoiceSettingsRepo;
use crate::repo::postgres::import_legacy_settings_once;
use crate::repo::traits::VoiceSessionsRepository;
use crate::repo::traits::VoiceSettingsRepository;
use crate::storage::DbPool;
use crate::storage::Store;

#[derive(Clone)]
pub struct Repository {
    pub voice_sessions: PgVoiceSessionsRepo,
    pub voice_settings: PgVoiceSettingsRepo,
    store: Store,
}

impl Repository {
    pub async fn connect(db_url: impl Into<String>) -> anyhow::Result<Self> {
        let store = Store::connect(db_url).await?;
        let pool = store.pool().clone();
        Ok(Self {
            voice_sessions: PgVoiceSessionsRepo::new(pool.clone()),
            voice_settings: PgVoiceSettingsRepo::new(pool),
            store,
        })
    }

    pub async fn migrate(&self) -> anyhow::Result<Vec<String>> {
        self.store.migrate().await
    }

    pub fn pool(&self) -> &DbPool {
        self.store.pool()
    }

    pub async fn import_legacy_settings_once(&self) -> Result<u64, DatabaseError> {
        import_legacy_settings_once(self.pool()).await
    }

    pub async fn delete_all(&self) -> Result<(), DatabaseError> {
        self.voice_sessions.delete_all().await?;
        self.voice_settings.delete_all().await
    }
}
