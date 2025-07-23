use sqlx::SqlitePool;

pub struct BaseTable {
    pub pool: SqlitePool,
}

impl BaseTable {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}
