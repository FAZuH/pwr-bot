# AGENTS.md

This document provides guidelines for AI agents working on the pwr-bot Rust codebase.

## Project Overview

- **Type**: Discord bot with feed subscriptions and voice channel tracking
- **Language**: Rust (Edition 2024)
- **Database**: SQLite with SQLx
- **Framework**: Serenity + Poise for Discord integration
- **Async Runtime**: Tokio

## Development Commands

Use `./dev.sh` for most tasks:
```bash
./dev.sh format|lint|test|build|all
```

Standard `cargo` commands work as expected. Tests require `SQLX_OFFLINE=true` (handled in CI).

## Code Style Guidelines

- **Imports**: Group `std`, external, then crate-local (see `rustfmt.toml`). No `use crate::module::*;`.
- **Explicit Imports**: Always import items explicitly at the top of the file. Do not use `use crate::*;` anywhere in the code.
- **Formatting**: Standard Rust (4 spaces, trailing commas). 100 char line length.
- **Naming**: `PascalCase` types, `snake_case` functions/vars, `SCREAMING_SNAKE` consts.
- **Errors**: Use `anyhow` for app errors, `thiserror` for custom types. Suffix with `Error`.
- **Async**: Use `tokio::spawn`, `&self` (interior mutability), and `tokio::sync::Mutex`.
- **Logging**: Use `log` macros (`info!`, `debug!`) with context.
- **Testing**: `#[tokio::test]`. Use `tests/common/` for utilities.

## Adding Commands

Commands follow the **Command Pattern** using Poise. Top-level commands are aggregated in `src/bot/commands.rs`.

1. Create a command module or function.
2. Implement the command with `#[poise::command]`.
3. Add the command function call to the `Cogs` implementation in `src/bot/commands.rs`.

```rust
#[poise::command(slash_command)]
pub async fn my_command(ctx: Context<'_>) -> Result<(), Error> { /* ... */ }
```

In `src/bot/commands.rs`:
```rust
impl Cog for Cogs {
    fn commands(&self) -> Vec<Command<Data, Error>> {
        vec![
            // ...
            my_command(),
        ]
    }
}
```

### Command Groups
For commands with subcommands, use the `subcommands` attribute:

```rust
#[poise::command(
    slash_command,
    subcommands("subcommand_a", "subcommand_b")
)]
pub async fn parent_command(_ctx: Context<'_>) -> Result<(), Error> { Ok(()) }
```

## Creating UI Views (ViewEngine architecture)

Views use the `ViewEngine` system in `src/bot/views.rs`:
- `Action` - Trait for the action enum.
- `ViewRender<T>` - Renders components/embeds using `ActionRegistry`.
- `ViewHandler<T>` - Handles logic and returns `ViewCommand`.
- `ViewEngine<T, H>` - Runs the event loop.

See [`.opencode/skills/ui-views/SKILL.md`](.opencode/skills/ui-views/SKILL.md) for detailed documentation and patterns.

## Database Schema Changes

See [`.opencode/skills/db-schema/SKILL.md`](.opencode/skills/db-schema/SKILL.md) for guidelines on:
- Creating migrations with `cargo sqlx migrate add`
- Writing safe UP/DOWN migration scripts
- Modifying models and repository code

## Creating and Modifying Skills

When certain types of documentation or patterns become repetitive, create a skill in `.opencode/skills/<skill-name>/SKILL.md`.

### When to Create a Skill

Create a new skill when:
1. A complex workflow requires multiple sequential steps that are repeated across the codebase
2. Documentation in AGENTS.md exceeds ~50 lines for a single topic
3. A pattern has multiple components that need to be configured together
4. The same instructions are needed by multiple developers/sub-agents

### Skill Structure

```
.opencode/skills/<skill-name>/
├── SKILL.md          # Main skill documentation (required)
├── README.md         # Quick overview (optional)
├── router.sh         # CLI entry point if skill has commands (optional)
└── scripts/         # Script files if needed (optional)
```

### SKILL.md Frontmatter

Each skill must include YAML frontmatter:

```yaml
---
name: <skill-name>
description: Brief description of what this skill does and when to use it
---
```

### Guidelines for Writing Skills

1. **Keep it practical** - Focus on actionable steps, not theory
2. **Use examples** - Show real code patterns from the codebase
3. **Include prerequisites** - What must be done before using the skill
4. **Provide validation** - How to verify the result is correct
5. **Reference related skills** - Link to other relevant skills

### Modifying Existing Skills

Update a skill when:
- The codebase patterns change (e.g., new framework version)
- New best practices are discovered
- Common errors indicate the documentation needs clarification
- The skill needs to cover additional use cases

## Commit Conventions

See [`.opencode/skills/commit/SKILL.md`](.opencode/skills/commit/SKILL.md) for detailed commit conventions, types, user-facing commit rules (`u_` prefix), and examples.

## CI/CD

- GitHub Actions: format, build, clippy, tests. All must pass.

## Documentation

### Architecture Diagrams

Update `docs/diagrams/` (`.mmd` source) and export PNG.

## Past Mistakes

| Mistake | Solution |
|---------|----------|
| Stripping code documentation comments during refactoring | Preserve all `///` doc comments and `//!` module docs; never delete documentation when moving code |
| Not using conventional commit | Strictly follow AGENTS.md's commit guideline |
