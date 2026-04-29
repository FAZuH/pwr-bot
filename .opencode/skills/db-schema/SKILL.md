---
name: db-schema
description: Handle database schema changes using Diesel migrations. Supports creating new tables, modifying existing tables, adding columns, and implementing the corresponding model and repository changes with proper validation and safe migration practices.
---

# Database Schema Modification Skill

## Overview

This skill handles all database schema changes in the pwr-bot project using **Diesel** (with `diesel-async` and `deadpool`) on **PostgreSQL**. It provides a structured workflow for:
- Creating new tables
- Modifying existing tables (add/remove columns, indexes, constraints)
- Rolling back migrations safely
- Implementing corresponding model and repository changes

## Prerequisites

- PostgreSQL database with Diesel
- Cargo project with `diesel`, `diesel-async`, and `diesel_migrations` installed
- Migration files stored in `migrations/` directory
- `diesel.toml` configured for PostgreSQL

## Workflow

### Step 1: Analyze the Change

Before making any changes, understand:
- **What table is being modified?**
- **What type of change is needed?** (new table, add column, remove column, add index, etc.)
- **Is the change backwards compatible?**
- **What data transformation is needed?**

### Step 2: Create Migration Using Diesel

Use `diesel migration generate` to create migration files:

```bash
# Create a new migration
diesel migration generate <migration_name>

# Example: Add a new user_preferences table
diesel migration generate add_user_preferences
```

This creates:
- `migrations/YYYYMMDDHHMMSS_add_user_preferences/up.sql`
- `migrations/YYYYMMDDHHMMSS_add_user_preferences/down.sql`

### Step 3: Write the UP Migration

The `up.sql` file should:
- Add the new table/column/index
- Include data migration if needed
- Be idempotent where possible (use `IF NOT EXISTS`)

```sql
-- Example: Creating a new table
CREATE TABLE IF NOT EXISTS user_preferences (
    id SERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    theme TEXT DEFAULT 'dark',
    notifications_enabled BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id)
);

-- Example: Adding a column to existing table
ALTER TABLE feeds ADD COLUMN IF NOT EXISTS last_fetched_at TIMESTAMPTZ DEFAULT NULL;
```

### Step 4: Write the DOWN Migration

The `down.sql` file should:
- Exactly reverse the UP migration
- Be precise to avoid data loss

```sql
-- Example: Drop the table created in UP
DROP TABLE IF EXISTS user_preferences;

-- Example: Remove the column added in UP
ALTER TABLE feeds DROP COLUMN IF EXISTS last_fetched_at;
```

### Step 5: Validate Migration Syntax

Run the migration against a test database to verify:

```bash
# Test migration locally (ensure PostgreSQL is running)
diesel migration run --database-url postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot

# Or verify without running
diesel migration list --database-url postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot
```

### Step 6: Regenerate Schema (if needed)

After adding new tables, update `src/repo/schema.rs`:

```bash
# Requires a running PostgreSQL instance with the migrations applied
diesel print-schema --database-url postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot > src/repo/schema.rs
```

Then manually verify:
- Auto-increment PKs use `Integer` (not `Nullable<Integer>`)
- Boolean columns use `Bool` (not `Integer`)
- Timestamps use `Timestamptz` (not `Timestamp`)
- JSON columns use `Jsonb` (not `Text`)

### Step 7: Modify Entity (src/entity.rs)

Add or update entity structs with Diesel derives:

```rust
use diesel::prelude::*;
use serde::Serialize;
use serde::Deserialize;
use chrono::DateTime;
use chrono::Utc;

#[derive(Queryable, Selectable, Insertable, Identifiable, Serialize, Deserialize, Default, Clone, Debug)]
#[diesel(table_name = user_preferences)]
#[diesel(primary_key(id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UserPreferencesEntity {
    #[serde(default)]
    pub id: i32,
    pub user_id: i64,
    pub theme: String,
    pub notifications_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

**Model Naming Conventions:**
- Table: `user_preferences` → Entity: `UserPreferencesEntity`
- Use `DbU64` newtype for Discord snowflakes (`u64`) stored as `BIGINT`
- Use `Json<T>` newtype for JSONB columns
- Use `DateTime<Utc>` for timestamps (Diesel PostgreSQL maps `Timestamptz` to `DateTime<Utc>`)

**For u64 IDs:**
```rust
use crate::entity::DbU64;

#[derive(Queryable, Selectable, Insertable)]
#[diesel(table_name = server_settings)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerSettingsEntity {
    pub guild_id: DbU64,
    // ...
}
```

**For JSONB columns:**
```rust
use crate::entity::Json;

pub struct ServerSettingsEntity {
    pub guild_id: DbU64,
    pub settings: Json<ServerSettings>,
}
```

### Step 8: Modify Repository Table (src/repo/table.rs)

Add table implementation implementing the appropriate trait from `src/repo/traits.rs`:

```rust
use diesel::prelude::*;
use diesel_async::RunQueryDsl;

use crate::entity::UserPreferencesEntity;
use crate::repo::error::DatabaseError;
use crate::repo::schema::user_preferences;
use crate::repo::traits::UserPreferencesRepository;

pub struct UserPreferencesTable {
    pool: DbPool,
}

impl UserPreferencesTable {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserPreferencesRepository for UserPreferencesTable {
    async fn insert(&self, model: &UserPreferencesEntity) -> Result<i32, DatabaseError> {
        let mut conn = self.pool.get().await?;
        let id = diesel::insert_into(user_preferences::table)
            .values(model)
            .returning(user_preferences::id)
            .get_result(&mut conn)
            .await?;
        Ok(id)
    }

    // ... implement other trait methods ...
}
```

### Step 9: Register Table in Repository (src/repo/mod.rs)

Add the new table to the `Repository` struct:

```rust
pub struct Repository {
    pool: DbPool,
    db_url: String,
    pub feed: FeedTable,
    pub feed_item: FeedItemTable,
    pub subscriber: SubscriberTable,
    pub feed_subscription: FeedSubscriptionTable,
    pub server_settings: ServerSettingsTable,
    pub voice_sessions: VoiceSessionsTable,
    pub bot_meta: BotMetaTable,
    // Add new table here
    pub user_preferences: UserPreferencesTable,
}

impl Repository {
    pub async fn new(db_url: impl Into<String>) -> anyhow::Result<Self> {
        // ... existing code ...
        
        let user_preferences = UserPreferencesTable::new(pool.clone());
        
        Ok(Self {
            // ... existing fields ...
            user_preferences,
        })
    }
    
    // Update drop_all_tables and delete_all_tables if needed
    pub async fn drop_all_tables(&self) -> anyhow::Result<()> {
        // ... existing tables ...
        self.user_preferences.drop_table().await?;
        Ok(())
    }
}
```

### Step 10: Add Entity Imports

Ensure `src/repo/table.rs` imports the new entity:

```rust
use crate::entity::UserPreferencesEntity;
```

## Migration Safety Guidelines

### 1. Always Use Transactions for Data Migrations

Diesel migrations run inside transactions by default. For manual SQL:

```sql
BEGIN;

-- Your migration steps

COMMIT;
-- Or on error:
ROLLBACK;
```

### 2. Validate Before Modifying

```sql
-- Check if column exists before adding
SELECT column_name 
FROM information_schema.columns 
WHERE table_name = 'table_name' AND column_name = 'column_name';

-- Check if index exists before creating
SELECT indexname 
FROM pg_indexes 
WHERE tablename = 'table_name' AND indexname = 'index_name';
```

### 3. Handle Existing Data

```sql
-- Add column with default value for existing rows
ALTER TABLE feeds ADD COLUMN new_column TEXT DEFAULT 'default_value';

-- Migrate existing data before changing constraints
UPDATE table_name SET new_field = old_field WHERE new_field IS NULL;
```

### 4. Use Safe Rollback Patterns

```sql
-- UP: Add column
ALTER TABLE users ADD COLUMN email TEXT NOT NULL DEFAULT '';

-- DOWN: Drop column
ALTER TABLE users DROP COLUMN email;
```

### 5. PostgreSQL Features

- Use `IF NOT EXISTS` for idempotent CREATE statements
- Use `IF EXISTS` for idempotent DROP statements
- `SERIAL` for auto-incrementing primary keys
- `TIMESTAMPTZ` for timezone-aware timestamps
- `JSONB` for JSON data (more efficient than `JSON`)
- `BOOLEAN` for true/false values

## Validation Checklist

Before completing the schema change:

- [ ] Migration files created with `diesel migration generate`
- [ ] UP migration tested locally against PostgreSQL
- [ ] DOWN migration tested (can rollback)
- [ ] `schema.rs` regenerated and manually corrected (nullability, types)
- [ ] Entity added/updated in `src/entity.rs`
- [ ] Table implementation added in `src/repo/table.rs`
- [ ] Repository updated in `src/repo/mod.rs`
- [ ] Build passes: `cargo build`
- [ ] Lint passes: `./dev.sh lint`
- [ ] Tests pass: `cargo test --all-features -- --test-threads=1`

## Quick Reference

| Task | Command |
|------|---------|
| Create migration | `diesel migration generate <name>` |
| Run migrations | `diesel migration run --database-url <url>` |
| Revert last | `diesel migration redo --database-url <url>` |
| List migrations | `diesel migration list --database-url <url>` |
| Print schema | `diesel print-schema --database-url <url>` |

## Error Handling

If migration fails:
1. Check SQL syntax (PostgreSQL syntax, not SQLite)
2. Verify table/column exists
3. Ensure foreign key constraints are valid
4. Check PostgreSQL version compatibility
5. Verify `diesel_cli` was compiled with PostgreSQL support

## Data Migration

For migrating from SQLite to PostgreSQL:

```bash
# Ensure PostgreSQL is running and DB_URL is set
export DB_URL="postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot"

# Run the migration script
uv run --script scripts/migrate.py
```
