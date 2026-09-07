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
- Tests need `DB_URL` in `.env` locally; CI copies `.env-example` → `.env` automatically
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

## UI Views (TEA host runtime)

Interactive views follow the Elm Architecture (see `docs/adr/0005-tea-gui-architecture.md`):

- **Core** `src/update/<feature>.rs` — pure `fn update(msg, &mut model) -> Vec<Effect>`; imports no serenity/tokio/diesel/poise (image bytes OK, serenity types not)
- **Shell** `src/bot/gui/` — sealed `GuiFeature` trait (pure `view`, `translate`, `attachments`, `open_modal` hook) + `Host` event loop in `rt.rs`
- **Adapters** — per-feature `EffectHandler` impls executing effects against `ctx.data().service`
- Interaction substrate (Action, ActionRegistry, SelectValues, ViewChannel, ViewEvent) lives in `src/bot/view/` — the collectors the Host runs on
- Snapshot tests in each feature pin rendered component JSON (custom_id timestamps normalized) — run them when touching any view

## Business Logic (Update Pattern)

Pure, testable state mutations live in `src/update/<feature>.rs` as free
`fn update(msg, &mut Model) -> Vec<Effect>` functions (plus `Model`, `Msg`,
`Effect` vocabularies and unit tests). Handlers in `src/bot/command/` parse
Discord interactions into `Msg`s, and side effects execute only through the
feature's `EffectHandler` adapter. See `docs/adr/0005-tea-gui-architecture.md`
for the layering rules. Existing modules: `about`, `feed_batch`,
`register`, `unregister`, `feed_list`, `voice_stats`,
`voice_leaderboard`, `plugins`, `pagination`.

- Place pure logic in `src/update/<feature>.rs` (Model, Msg, Effect, `update` fn, tests)
- Handlers in `src/bot/command/` parse Discord interactions into `Msg`s, run `update`, and route returned `Effect`s to the feature's `EffectHandler` adapter (never execute effects inline)

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

## Agent skills

### Issue tracker

Issues live as GitHub issues; use `gh`. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context — root `CONTEXT.md` + `docs/adr/`. See `docs/agents/domain.md`.
