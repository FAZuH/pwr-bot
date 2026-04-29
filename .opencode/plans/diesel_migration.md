# Refactor Plan: SQLx → Diesel for `pwr-bot/src/repo/`

## Executive Summary

Migrate the entire persistence layer (`src/repo/`, `src/entity.rs`, migrations, tests, and docs) from **SQLx** to **Diesel**, using the `tomo` project as the canonical reference for Diesel configuration, schema generation, model derives, and migration embedding.

> **Critical Architecture Decision Required**  
> Diesel’s SQLite driver is **synchronous**; `pwr-bot` is fully async (Tokio + Serenity + Poise). You must choose **Option A** or **Option B** below before implementation begins. This decision ripples through every layer.

---

## Decision Point: Sync vs. Async Diesel

| | **Option A: `diesel-async`** (Recommended) | **Option B: Sync Diesel + `spawn_blocking`** (Matches `tomo`) |
|---|---|---|
| **Crate** | `diesel-async` + `diesel-async/deadpool` (or `bb8`) | `diesel` + `diesel/r2d2` |
| **Pool** | `deadpool::Pool<AsyncSqliteConnection>` | `r2d2::Pool<ConnectionManager<SqliteConnection>>` |
| **Repo Traits** | Keep `#[async_trait]`; swap `sqlx::Error` → `diesel::result::Error` | Remove `#[async_trait]`; make all methods sync |
| **Service/Command Layer** | **Minimal changes** — signatures stay async | **Major changes** — wrap every DB call in `tokio::task::spawn_blocking` |
| **Test Harness** | `#[tokio::test]` stays unchanged | Tests become sync or use `spawn_blocking` |
| **Migration Run** | `async` via `diesel_async_migrations` | Sync via `diesel_migrations::MigrationHarness::run_pending_migrations` |
| **Complex Raw SQL** | `diesel::sql_query` works via `AsyncConnection` | `diesel::sql_query` works via `SqliteConnection` |
| **Drawback** | Extra dependency; `diesel-async` SQLite support is newer | Heavy boilerplate; risk of blocking the Tokio runtime if forgotten |

**Recommendation:** Choose **Option A (`diesel-async`)**. It preserves the existing async trait signatures and avoids `spawn_blocking` boilerplate across ~30+ service/command methods. If you prefer Option B, add a `spawn_blocking` helper in `src/repo/mod.rs` and use it religiously.

---

## Phase 1: Tooling, Dependencies & Migration Infrastructure

### 1.1 `Cargo.toml`
```toml
# REMOVE
sqlx = { version = "0.8.6", features = ["chrono", "runtime-tokio", "sqlite"] }

# ADD (Option A — diesel-async)
diesel = { version = "2.3", features = ["chrono", "returning_clauses_for_sqlite_3_35", "sqlite"] }
diesel-async = { version = "0.5", features = ["deadpool", "sqlite"] }
diesel-async-migrations = "0.2"

# OR ADD (Option B — sync diesel)
diesel = { version = "2.3", features = ["chrono", "r2d2", "returning_clauses_for_sqlite_3_35", "sqlite"] }
diesel_migrations = "2.3"
```
Also remove any indirect `sqlx` dev-dependencies if they exist.

### 1.2 `diesel.toml` (new file, root)
Copy from `tomo` and adapt:
```toml
[print_schema]
file = "src/repo/schema.rs"
with_docs = true
custom_type_derives = ["diesel::query_builder::QueryId", "Clone"]

[migrations_directory]
dir = "migrations"
```

### 1.3 Migration File Reformatting
`pwr-bot` migrations are flat files (`migrations/20250801211447_initial_setup.up.sql`). Diesel expects subdirectories:
```
migrations/
  2025-08-01-211447_initial_setup/
    up.sql
    down.sql
```

**Strategy:**
1. Create a **single consolidated initial migration** capturing the *current* schema state (merge all existing `.up.sql` files in order).
2. Archive old SQLx-format migrations in `migrations/_archive/` for reference.
3. Future migrations use `diesel migration add <name>`.

> **Production Safety:** Existing deployed databases have already run SQLx migrations. The new Diesel migration must only run on fresh databases (CI, new dev clones, tests). Do **not** attempt to re-run schema changes on existing DBs.

### 1.4 CI Updates (`.github/workflows/pull-request.yml`)
- Remove `SQLX_OFFLINE: true` env var.
- Remove any `.sqlx/` caching steps (none currently, but check).
- Ensure `DATABASE_URL` or a temp DB path is available for tests (Diesel migrations need a real SQLite file at runtime, not compile-time metadata).

---

## Phase 2: Schema & Domain Models

### 2.1 Generate `src/repo/schema.rs`
```bash
diesel setup      # creates DB if missing
diesel migration run
diesel print-schema > src/repo/schema.rs
```
Register `pub mod schema;` in `src/repo/mod.rs`.

### 2.2 Refactor `src/entity.rs` (Domain Layer)
This is the highest-impact file. Remove all `sqlx` derives and types.

| Current SQLx Pattern | Diesel Replacement |
|---|---|
| `#[derive(FromRow, …)]` | `#[derive(Queryable, Selectable, Insertable, Identifiable, …)]` |
| `#[sqlx(try_from = "i64")]` | Custom newtype `DbU64` or cast in repo layer (see 2.3) |
| `sqlx::types::Json<ServerSettings>` | Custom `Json<T>` newtype implementing `FromSql<Text, Sqlite>` / `ToSql<Text, Sqlite>` (see 2.4) |
| `#[derive(sqlx::Type)]` on `SubscriberType` | Manual `FromSql<Text, Sqlite>` / `ToSql<Text, Sqlite>` impl (mirror `tomo`’s `PomodoroState`) |

**Example transformation (`FeedEntity`):**
```rust
// BEFORE
#[derive(FromRow, Serialize, Deserialize, Default, Clone, Debug)]
pub struct FeedEntity { … }

// AFTER
#[derive(Queryable, Selectable, Insertable, Identifiable, Serialize, Deserialize, Default, Clone, Debug)]
#[diesel(table_name = crate::repo::schema::feeds)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct FeedEntity {
    pub id: i32,
    pub name: String,
    // …
}
```

### 2.3 `u64` ↔ `i64` Mapping
Discord snowflakes (`guild_id`, `user_id`, `channel_id`) are `u64`. SQLx handled this via `#[sqlx(try_from = "i64")]`. Diesel does not have this built-in.

**Recommended approach:**
- Create a newtype `pub struct DbU64(pub u64);` in `src/repo/mod.rs` or `src/entity.rs`.
- Implement `FromSql<BigInt, Sqlite>` and `ToSql<BigInt, Sqlite>` for `DbU64` that casts via `i64`.
- Use `DbU64` in models where `u64` is needed, or keep models as `i64` and cast at the service boundary (less invasive).

### 2.4 JSON Column Wrapper (`ServerSettings`)
`server_settings.settings` is a JSON TEXT blob.

**Implementation:**
```rust
#[derive(Clone, Debug, AsExpression, FromSqlRow)]
#[diesel(sql_type = Text)]
pub struct Json<T>(pub T);

impl<T: Serialize> ToSql<Text, Sqlite> for Json<T> { … }
impl<T: DeserializeOwned> FromSql<Text, Sqlite> for Json<T> { … }
```
Replace every `sqlx::types::Json(settings)` with `Json(settings)`.

### 2.5 Query-Only Structs
Structs like `FeedWithLatestItemRow`, `VoiceLeaderboardEntry`, `VoiceDailyActivity`, `GuildDailyStats` are not table models but raw SQL result mappers.

- Keep them in `src/entity.rs`.
- Replace `#[derive(FromRow)]` with `#[derive(QueryableByName)]` and annotate fields with `#[diesel(sql_type = …)]` for use with `diesel::sql_query`.

---

## Phase 3: Repository Traits (`src/repo/traits.rs`)

### 3.1 Remove `async_trait` (Option B only)
If Option A (`diesel-async`): keep `#[async_trait]` but remove `Send + Sync` bounds where redundant.

If Option B (sync): strip `#[async_trait]` and `async` from every method.

### 3.2 Error Type Update
Change `DatabaseError::BackendError(sqlx::Error)` → `DatabaseError::BackendError(diesel::result::Error)`.

### 3.3 Trait Object Safety
`Repository` uses `Box<dyn FeedRepository>`. Ensure traits remain object-safe:
- No generic methods.
- No `impl Trait` return types.
- Associated types only if necessary (avoid if possible).

---

## Phase 4: Repository Implementation (`src/repo/table.rs`)

### 4.1 Delete Legacy Infrastructure
- Remove `BaseTable`.
- Remove `BindParam` trait and all `impl_bind_param!` macro invocations.
- Remove `impl_table!` macro entirely.

### 4.2 Table Structs
Each table becomes a plain struct holding a pool reference (or nothing, if the pool is passed per-call). Following `tomo`:

```rust
#[derive(Clone)]
pub struct FeedTable {
    pool: DbPool, // deadpool (Option A) or r2d2 (Option B)
}
```

### 4.3 CRUD Implementations (Diesel DSL)
Replace macro-generated SQLx code with explicit Diesel methods.

**Example (`FeedTable::insert`):**
```rust
use crate::repo::schema::feeds::dsl::*;

async fn insert(&self, model: &FeedEntity) -> Result<i32, DatabaseError> {
    diesel::insert_into(feeds)
        .values(model)
        .returning(id)
        .get_result(&self.pool)
        .await
        .map_err(DatabaseError::from)
}
```

> **Note:** `returning_clauses_for_sqlite_3_35` feature is required for `.returning()` on SQLite.

### 4.4 Complex Raw SQL (Voice Leaderboard Queries)
The voice leaderboard queries use `strftime`, `MIN`, `MAX`, window clipping, self-JOINs, and conditional SQL building. These cannot be expressed in Diesel’s typed DSL easily.

**Strategy:** Keep them as raw SQL using `diesel::sql_query`.

```rust
let rows: Vec<VoiceLeaderboardEntry> = diesel::sql_query(r#"SELECT …"#)
    .bind::<Text, _>(until_val.to_rfc3339())
    .load(&self.pool)
    .await?;
```

Ensure `VoiceLeaderboardEntry` derives `QueryableByName` and each field has `#[diesel(sql_type = …)]`.

### 4.5 `REPLACE INTO` → `INSERT … ON CONFLICT`
SQLx uses `REPLACE INTO` in the `impl_table!` macro. Diesel idiomatically uses:
```rust
diesel::insert_into(table)
    .values(model)
    .on_conflict(id)
    .do_update_set(model)
    .execute(&conn)?;
```
Or, if the `REPLACE` semantics are strictly needed, keep a raw `sql_query("REPLACE INTO …")`.

---

## Phase 5: Repository Root (`src/repo/mod.rs`)

### 5.1 Pool & Connection Setup
```rust
// Option A (diesel-async + deadpool)
pub type DbPool = deadpool::Pool<AsyncSqliteConnection>;

pub struct Repository {
    pool: DbPool,
    pub feed: Box<dyn FeedRepository>,
    // …
}

// Option B (sync diesel + r2d2)
pub type DbPool = r2d2::Pool<ConnectionManager<SqliteConnection>>;
```

### 5.2 Migrations
Replace:
```rust
sqlx::migrate!("./migrations").run(&self.pool).await?;
```
With:
```rust
// Option A
use diesel_async_migrations::{embed_migrations, EmbeddedMigrations};
const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
// run inside an async connection

// Option B
use diesel_migrations::{embed_migrations, EmbeddedMigrations};
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
conn.run_pending_migrations(MIGRATIONS)?;
```

### 5.3 `drop_all_tables` / `delete_all_tables`
Rewrite using `diesel::sql_query` or schema DSL `table.delete().execute(&conn)?`.

---

## Phase 6: Error Handling (`src/repo/error.rs`)

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DatabaseError {
    #[error("Database error: {0}")]
    BackendError(#[from] diesel::result::Error),
    // …
}
```

---

## Phase 7: Service Layer Updates

### 7.1 `src/service/feed_subscription.rs`
Replace `sqlx::error::ErrorKind::UniqueViolation` detection:
```rust
// BEFORE
use sqlx::error::ErrorKind;
matches!(db_err.kind(), ErrorKind::UniqueViolation)

// AFTER
use diesel::result::{Error as DieselError, DatabaseErrorKind};
matches!(err, DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _))
```

### 7.2 `src/service/settings.rs`
Replace `sqlx::types::Json(settings)` with the custom `Json(settings)` newtype.

---

## Phase 8: Command / Controller Layer

### 8.1 Files to update
- `src/bot/command/settings.rs` (line 113)
- `src/bot/command/gui_test/steps/settings.rs` (line 38)

Change:
```rust
settings: sqlx::types::Json(settings),
// →
settings: Json(settings),
```

---

## Phase 9: Tests

### 9.1 `tests/common.rs`
Update `setup_db`:
```rust
pub async fn setup_db() -> (Arc<Repository>, PathBuf) {
    let db_path = …;
    let db = Repository::new(&db_url, db_path.to_str().unwrap())
        .await
        .expect("Failed to create database");
    db.run_migrations().await.expect("Failed to run migrations");
    (Arc::new(db), db_path)
}
```
(If Option B, `Repository::new` may become synchronous.)

### 9.2 `tests/db_table.rs` & `tests/voice_tracking_service.rs`
Replace `sqlx::types::Json(ServerSettings { … })` with `Json(ServerSettings { … })`.

### 9.3 `tests/feed_subscription_service.rs` & `tests/publisher_subscriber.rs`
Verify no direct `sqlx` imports; if any, replace with Diesel equivalents.

---

## Phase 10: Documentation & Skills

### 10.1 `AGENTS.md`
Update the **Database** section:
```markdown
## Database

- SQLite with Diesel (ORM + query builder)
- Migrations: `diesel migration add <name>` / `diesel migration run`
- Async pool: `deadpool` via `diesel-async` (Option A) or `r2d2` (Option B)
- Schema auto-generated: `diesel print-schema > src/repo/schema.rs`
- See `.opencode/skills/db-schema/SKILL.md` for migration and model patterns
```
Also update the **Project** bullet: `Rust 2024, SQLite + Diesel, Serenity + Poise, Tokio`.

### 10.2 `.opencode/skills/db-schema/SKILL.md`
Complete rewrite:
- Replace `cargo sqlx migrate add` with `diesel migration add <name>`.
- Replace `#[derive(FromRow)]` examples with `#[derive(Queryable, Selectable, Insertable)]`.
- Replace `sqlx::types::Json` guidance with custom Diesel JSON newtype pattern.
- Replace `.sqlx/` offline query metadata guidance with `diesel print-schema` workflow.
- Update validation checklist: `cargo build` / `cargo test` (no `SQLX_OFFLINE`).
- Add Diesel-specific safety notes (SQLite `RETURNING` requires 3.35+, `ON CONFLICT` syntax).

### 10.3 `docs/diagrams/architecture-layers.mmd`
Change:
```
SQLite["SQLite  ·  SQLx async"]
→
SQLite["SQLite  ·  Diesel async"]   // or just "SQLite  ·  Diesel"
```

### 10.4 `docs/diagrams/architecture-patterns.mmd`
Change:
```
DB["SQLite / SQLx"]
→
DB["SQLite / Diesel"]
```

### 10.5 `docs/architecture.md`
Update line 20 and line 163:
```
repository/ — SQLite via SQLx
→
repository/ — SQLite via Diesel
```

### 10.6 Re-export Diagrams
```bash
./dev.sh docs
```

---

## Phase 11: Cleanup & Verification

1. `cargo check` — fix compilation errors iteratively.
2. `cargo test --all-features` — all DB tests must pass.
3. `cargo clippy --all-targets --all-features` — zero warnings.
4. `./dev.sh format lint` — final formatting.
5. Delete any orphaned `.sqlx/` directory if it appears.
6. Commit with `[pub]` if this is user-facing, or plain `refactor` if internal.

---

## Risk Register

| Risk | Mitigation |
|---|---|
| **Blocking Tokio runtime** (Option B) | Use `spawn_blocking` helper; audit every repo call. Prefer Option A. |
| **u64/i64 casting regressions** | Centralize in `DbU64` newtype; add unit tests for boundary values. |
| **Raw SQL query breakage** | Keep voice leaderboard queries in `sql_query` with `QueryableByName`; integration test them. |
| **Migration divergence** | Single consolidated initial migration; archive old ones. Test fresh DB creation in CI. |
| **JSON serialization drift** | Custom `Json<T>` newtype must use the same serde settings as before; test round-trip. |
| **`returning` on old SQLite** | Ensure `returning_clauses_for_sqlite_3_35` feature is enabled; verify CI SQLite version ≥ 3.35. |

---

## Appendix: File Checklist

| File | Action |
|---|---|
| `Cargo.toml` | Swap `sqlx` deps for `diesel` (+ async if Option A) |
| `diesel.toml` | **Create** (copy from `tomo`) |
| `migrations/` | Reformat to Diesel subdirs; consolidate initial schema |
| `src/repo/schema.rs` | **Create** via `diesel print-schema` |
| `src/repo/mod.rs` | Rewrite pool & migration logic |
| `src/repo/error.rs` | Replace `sqlx::Error` with `diesel::result::Error` |
| `src/repo/traits.rs` | Update for Diesel; optionally remove `async_trait` |
| `src/repo/table.rs` | **Complete rewrite** — remove macros, implement Diesel DSL |
| `src/entity.rs` | Replace SQLx derives with Diesel derives; add custom types |
| `src/service/feed_subscription.rs` | Update `UniqueViolation` check |
| `src/service/settings.rs` | Replace `sqlx::types::Json` |
| `src/bot/command/settings.rs` | Replace `sqlx::types::Json` |
| `src/bot/command/gui_test/steps/settings.rs` | Replace `sqlx::types::Json` |
| `tests/common.rs` | Update DB setup for Diesel pool |
| `tests/db_table.rs` | Update JSON wrapper usage |
| `tests/voice_tracking_service.rs` | Update JSON wrapper usage |
| `.github/workflows/pull-request.yml` | Remove `SQLX_OFFLINE` |
| `AGENTS.md` | Update Database section & project summary |
| `.opencode/skills/db-schema/SKILL.md` | Full rewrite for Diesel workflow |
| `docs/diagrams/*.mmd` | Update SQLx → Diesel labels |
| `docs/architecture.md` | Update SQLx → Diesel references |
