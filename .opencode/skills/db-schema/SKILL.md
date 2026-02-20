---
name: db-schema
description: Handle database schema changes using SQLx migrations. Supports creating new tables, modifying existing tables, adding columns, and implementing the corresponding model and repository changes with proper validation and safe migration practices.
---

# Database Schema Modification Skill

## Overview

This skill handles all database schema changes in the pwr-bot project. It provides a structured workflow for:
- Creating new tables
- Modifying existing tables (add/remove columns, indexes, constraints)
- Rolling back migrations safely
- Implementing corresponding model and repository changes

## Prerequisites

- SQLite database with SQLx
- Cargo project with `sqlx` crate installed
- Migration files stored in `migrations/` directory

## Workflow

### Step 1: Analyze the Change

Before making any changes, understand:
- **What table is being modified?**
- **What type of change is needed?** (new table, add column, remove column, add index, etc.)
- **Is the change backwards compatible?**
- **What data transformation is needed?**

### Step 2: Create Migration Using SQLx

Use `sqlx migrate add` to create migration files:

```bash
# Create a new migration
cargo sqlx migrate add -r <migration_name>

# Example: Add a new user_preferences table
cargo sqlx migrate add -r add_user_preferences
```

This creates:
- `migrations/YYYYMMDDHHMMSS_add_user_preferences.up.sql`
- `migrations/YYYYMMDDHHMMSS_add_user_preferences.down.sql`

### Step 3: Write the UP Migration

The `.up.sql` file should:
- Add the new table/column/index
- Include data migration if needed
- Be idempotent where possible (use `IF NOT EXISTS`)

```sql
-- Example: Creating a new table
CREATE TABLE IF NOT EXISTS user_preferences (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    theme TEXT DEFAULT 'dark',
    notifications_enabled INTEGER DEFAULT 1,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id)
);

-- Example: Adding a column to existing table
ALTER TABLE feeds ADD COLUMN last_fetched_at TIMESTAMP DEFAULT NULL;
```

### Step 4: Write the DOWN Migration

The `.down.sql` file should:
- Exactly reverse the UP migration
- Be precise to avoid data loss

```sql
-- Example: Drop the table created in UP
DROP TABLE IF EXISTS user_preferences;

-- Example: Remove the column added in UP
-- Note: SQLite has limited ALTER TABLE support
-- For complex rollbacks, may need to recreate table with old schema
```

### Step 5: Validate Migration Syntax

Run the migration against a test database to verify:

```bash
# Test migration locally
DATABASE_URL="sqlite:data/test.db" cargo sqlx migrate run

# Or verify without running
cargo sqlx migrate info
```

### Step 6: Modify Model (src/model.rs)

Add or update model structs:

```rust
// New model struct
#[derive(FromRow, Serialize, Default, Clone, Debug)]
pub struct UserPreferencesModel {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub user_id: i32,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub notifications_enabled: bool,
    #[serde(default)]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub updated_at: DateTime<Utc>,
}

// For u64 IDs, use sqlx type conversion
#[derive(FromRow, Serialize, Default, Clone, Debug)]
pub struct SomeModel {
    #[serde(default)]
    #[sqlx(try_from = "i64")]
    pub user_id: u64,
    // ... other fields
}
```

**Model Naming Conventions:**
- Table: `user_preferences` → Model: `UserPreferencesModel`
- Use `#[sqlx(try_from = "i64")]` for u64 to i64 conversions
- Include `#[serde(default)]` for optional fields

### Step 7: Modify Repository Table (src/repository/table.rs)

Add table implementation using `impl_table!` macro or custom methods:

```rust
// Using impl_table! macro for standard CRUD
impl_table!(
    UserPreferencesTable,
    UserPreferencesModel,
    "user_preferences",
    id,
    i32,
    i32,
    r#"CREATE TABLE IF NOT EXISTS user_preferences (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        user_id INTEGER NOT NULL,
        theme TEXT DEFAULT 'dark',
        notifications_enabled INTEGER DEFAULT 1,
        created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
        UNIQUE(user_id)
    )"#,
    "user_id, theme, notifications_enabled, created_at, updated_at",
    "?, ?, ?, ?, ?",
    "user_id = ?, theme = ?, notifications_enabled = ?, created_at = ?, updated_at = ?",
    [user_id, theme, notifications_enabled, created_at, updated_at]
);

// Add custom methods if needed
impl UserPreferencesTable {
    pub async fn select_by_user_id(&self, user_id: i32) -> Result<Option<UserPreferencesModel>, DatabaseError> {
        Ok(sqlx::query_as::<_, UserPreferencesModel>(
            "SELECT * FROM user_preferences WHERE user_id = ?"
        )
        .bind(user_id)
        .fetch_optional(&self.base.pool)
        .await?)
    }
}
```

### Step 8: Register Table in Repository (src/repository.rs)

Add the new table to the Repository struct:

```rust
pub struct Repository {
    pool: SqlitePool,
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
    pub async fn new(db_url: &str, db_path: &str) -> anyhow::Result<Self> {
        // ... existing code ...
        
        // Initialize new table
        let bot_meta = BotMetaTable::new(pool.clone());
        let user_preferences = UserPreferencesTable::new(pool.clone());
        
        Ok(Self {
            pool,
            feed,
            feed_item,
            subscriber,
            feed_subscription,
            server_settings,
            voice_sessions,
            bot_meta,
            // Add to struct
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

### Step 9: Add Model Imports

Ensure `src/repository/table.rs` imports the new model:

```rust
use crate::model::UserPreferencesModel;
```

## Migration Safety Guidelines

### 1. Always Use Transactions for Data Migrations

```sql
BEGIN TRANSACTION;

-- Your migration steps

COMMIT;
-- Or on error:
ROLLBACK;
```

### 2. Validate Before Modifying

```sql
-- Check if column exists before adding
SELECT COUNT(*) FROM pragma_table_info('table_name') WHERE name = 'column_name';

-- Check if index exists before creating
SELECT name FROM sqlite_master WHERE type='index' AND name='index_name';
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
-- UP: Add NOT NULL column
ALTER TABLE users ADD COLUMN email TEXT NOT NULL;

-- DOWN: Make nullable first, then drop
ALTER TABLE users ALTER COLUMN email TEXT NULL;
ALTER TABLE users DROP COLUMN email;
```

### 5. SQLite Limitations

- No `ALTER TABLE DROP COLUMN` in older SQLite (use `ALTER TABLE RENAME TO`)
- No `ALTER TABLE ADD CONSTRAINT` - recreate table for constraints
- Use `IF NOT EXISTS` and `IF EXISTS` for idempotency

## Validation Checklist

Before completing the schema change:

- [ ] Migration files created with `sqlx migrate add`
- [ ] UP migration tested locally
- [ ] DOWN migration tested (can rollback)
- [ ] Model added/updated in `src/model.rs`
- [ ] Table implementation added in `src/repository/table.rs`
- [ ] Repository updated in `src/repository.rs`
- [ ] Build passes: `cargo build`
- [ ] Lint passes: `./dev.sh lint`
- [ ] Tests pass: `./dev.sh test`

## Quick Reference

| Task | Command |
|------|---------|
| Create migration | `cargo sqlx migrate add -r <name>` |
| Run migrations | `cargo sqlx migrate run` |
| Revert last | `cargo sqlx migrate revert` |
| List migrations | `cargo sqlx migrate info` |

## Error Handling

If migration fails:
1. Check SQL syntax
2. Verify table/column exists
3. Ensure foreign key constraints are valid
4. For SQLite, check version compatibility
5. Use `PRAGMA foreign_keys = ON` to enforce FK constraints
