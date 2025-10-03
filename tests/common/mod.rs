use pwr_bot::database::database::Database;
use std::sync::Arc;

pub async fn get_in_memory_db() -> Arc<Database> {
    let db = Arc::new(
        Database::new("sqlite::memory:", "test.db")
            .await
            .expect("Failed to create in-memory database"),
    );
    db.create_all_tables()
        .await
        .expect("Failed to create tables");
    db
}
