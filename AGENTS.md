# AGENTS.md

Guidelines for AI agents working on the pwr-bot Rust codebase.

## Project

- Discord bot with feed subscriptions and voice channel tracking
- Rust 2024, PostgreSQL + Diesel (diesel-async + deadpool), Serenity + Poise, Tokio
- Requires **nightly Rust** for formatting (`rustfmt.toml` uses `imports_granularity = "Item"`)

## Development Commands

```bash
# Quick iteration — compilation check only, does not modify files
cargo check

# Run tests (locally needs a .env file; CI copies .env-example → .env automatically)
cargo test --all-features

# When finishing up — format + lint modifies files, so run last
./dev.sh format lint   # format uses +nightly; lint uses clippy --fix --allow-dirty
```

- Do **not** run `./dev.sh format lint` after every edit — it mutates source files and may require re-reading
- Do **not** use `./dev.sh build` for quick feedback — it builds a Docker image
- Tests need `DATABASE_URL` in `.env` locally; CI copies `.env-example` → `.env` automatically
- CI order: `fmt --check` → `build --all-targets` → `clippy -D warnings` → `test`
- Diagrams: always use `./dev.sh docs`, never invoke `mmdc` directly

## Code Style

- Imports: group `std`, external, then crate-local (`rustfmt.toml`). No `use crate::module::*;`
- Line length: 100 chars
- Errors: `anyhow` for app errors, `thiserror` for custom types (suffix `Error`)
- Async: `tokio::spawn`, `&self` with interior mutability, `tokio::sync::Mutex`
- Logging: `log` macros (`info!`, `debug!`)

## Adding Commands

1. Create module under `src/bot/command/`
2. Implement with `#[poise::command(slash_command)]`
3. Register in `src/bot/command/mod.rs` inside `Cogs::commands()`

```rust
// src/bot/command/my_module.rs
#[poise::command(slash_command)]
pub async fn my_command(ctx: Context<'_>) -> Result<(), Error> { /* ... */ }

// src/bot/command/mod.rs
impl Cog for Cogs {
    fn commands(&self) -> Vec<Command<Data, Error>> {
        vec![
            // ...
            my_module::my_command(),
        ]
    }
}
```

## UI Views (ViewEngine)

Interactive views live in `src/bot/view/` (formerly `src/bot/views.rs`).

- `ViewRender` and `ViewHandler` use **associated types**: `type Action: Action`
- `ViewEngine<T, H>` requires `H: ViewHandler<Action = T> + ViewRender<Action = T>`
- `ViewEvent` is non-generic; `ViewContext` carries `action: Option<T>` separately
- See `.opencode/skills/ui-views/SKILL.md` for full patterns

## Business Logic (Update Pattern)

Pure, testable state mutations follow the TEA `Update` trait in `src/update/mod.rs`:

```rust
pub trait Update {
    type Model;
    type Msg;
    type Cmd;
    fn update(msg: Self::Msg, model: &mut Self::Model) -> Self::Cmd;
}
```

- Place pure logic in `src/update/<feature>.rs` (Msg, Model, Cmd, Update impl, tests)
- Handlers in `src/bot/command/` parse Discord interactions, call `Update::update()`, then execute side effects (DB queries, image generation) based on the returned `Cmd`
- See existing modules: `voice_leaderboard`, `voice_stats`, `feed_list`, `welcome_settings`, `feed_settings`, `settings_main`

## Database

- PostgreSQL with Diesel (diesel-async 0.8 + deadpool)
- Migrations: `diesel migration generate <name>` (requires `diesel_cli` installed with PostgreSQL support)
- Schema source: `src/repo/schema.rs` — regenerate with `diesel print-schema` after migration changes, then manually correct `Nullable<Integer>` PKs to `Integer`
- See `.opencode/skills/db-schema/SKILL.md` for migration and model patterns
- Migration script: `scripts/migrate.py` (SQLite → PostgreSQL data migration)

## Commit Conventions

See `.opencode/skills/commit/SKILL.md` for full conventions.

- **User-facing commits**: include `[pub]` or `[public]` in the message (anywhere) to appear in the changelog
- **CI skip**: append `[skip ci]`, `[no ci]`, `[ci skip]`, etc. for docs/format-only commits
- **Version bumps**: use `chore!(major)` or `chore!(minor)` in the subject to trigger major/minor releases
- Do **not** use the old `u_` prefix — it has been replaced by the `[pub]` marker

## Architecture Diagrams

Source lives in `docs/diagrams/*.mmd`. Export to PNG with `mmdc` after edits.

## Past Mistakes

| Mistake | Solution |
|---------|----------|
| Stripping doc comments during refactoring | Preserve all `///` and `//!` docs when moving code |
| Wrong commit format | Follow `.opencode/skills/commit/SKILL.md` strictly |
