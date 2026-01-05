use std::sync::Arc;

use crate::database::Database;
use crate::database::model::VoiceSessionsModel;
use crate::database::table::Table;

pub struct VoiceTrackingService {
    db: Arc<Database>,
}

impl VoiceTrackingService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn insert(&self, model: &VoiceSessionsModel) -> anyhow::Result<()> {
        self.db.voice_sessions_table.insert(model).await?;
        Ok(())
    }
    pub async fn replace(&self, model: &VoiceSessionsModel) -> anyhow::Result<()> {
        self.db.voice_sessions_table.replace(model).await?;
        Ok(())
    }
}
