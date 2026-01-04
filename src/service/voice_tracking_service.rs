use std::sync::Arc;

use crate::database::Database;

pub struct VoiceTrackingService {
    db: Arc<Database>,
}

impl VoiceTrackingService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}
