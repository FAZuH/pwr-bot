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
- **Formatting**: Standard Rust (4 spaces, trailing commas). 100 char line length.
- **Naming**: `PascalCase` types, `snake_case` functions/vars, `SCREAMING_SNAKE` consts.
- **Errors**: Use `anyhow` for app errors, `thiserror` for custom types. Suffix with `Error`.
- **Async**: Use `tokio::spawn`, `&self` (interior mutability), and `tokio::sync::Mutex`.
- **Logging**: Use `log` macros (`info!`, `debug!`) with context.
- **Testing**: `#[tokio::test]`. Use `tests/common/` for utilities.

## Adding Commands

Commands use the **Cog pattern**:
1. Create a Cog struct implementing `Cog` trait.
2. Implement commands with `#[poise::command]`.

```rust
pub struct MyCog;
impl MyCog {
    #[poise::command(slash_command)]
    pub async fn cmd(ctx: Context<'_>) -> Result<(), Error> { /* ... */ }
}
impl Cog for MyCog {
    fn commands(&self) -> Vec<Command<Data, Error>> { vec![Self::cmd()] }
}
```

## Creating UI Views (Components V2)

Traits: `ViewProvider` -> `ResponseComponentView` -> `StatefulView` -> `InteractableComponentView`.

### Static View
Implement `ResponseComponentView` to return `Vec<CreateComponent>`.

### Interactive View
Use `stateful_view!` macro:

```rust
stateful_view! {
    timeout = Duration::from_secs(120),
    pub struct MyView<'a> { confirmed: bool }
}

impl ResponseComponentView for MyView<'_> { /* create_components */ }

#[async_trait]
impl<'a> InteractableComponentView<'a, MyAction> for MyView<'a> {
    async fn handle(&mut self, interaction: &ComponentInteraction) -> Option<MyAction> { /* ... */ }
}
```

### Key View Components
- **Containers**: `CreateContainer` - Groups components.
- **TextDisplay**: `CreateTextDisplay` - Markdown text.
- **Sections**: `CreateSection` - Side-by-side layout.
- **Buttons**: `CreateButton` - Interactive buttons.
- **Select Menus**: `CreateSelectMenu` - Dropdowns.

### CI/CD
- GitHub Actions: format, build, clippy, tests. All must pass.

## Design Patterns

### Controller Pattern (MVC-C)

Coordinator manages `Context` and message lifecycle. Controllers implement logic and return `NavigationResult`.

**Architecture**: `Coordinator` -> `Controller` -> `View` -> `NavigationResult`

```rust
// 1. Controller
controller! { pub struct MyController<'a> {} }

#[async_trait]
impl<S: Send + Sync + 'static> Controller<S> for MyController<'_> {
    async fn run(&mut self, coord: &mut Coordinator<'_, S>) -> Result<NavigationResult, Error> {
        let ctx = *coord.context();
        // logic ...
        coord.send(view.create_reply()).await?;
        Ok(NavigationResult::Exit)
    }
}

// 2. Coordinator Loop
pub async fn run(ctx: Context<'_>) -> Result<(), Error> {
    let mut coord = Coordinator::new(ctx);
    // ... loop calling controller.run(&mut coord) ...
}
```

**Guidelines**:
- **Navigation**: Use `NavigationResult` unified enum.
- **Context**: Clone context `let ctx = *coord.context()` to avoid borrows.
- **Recursion**: Avoid it; use the loop.

## Documentation

### Architecture Diagrams

Update `docs/diagrams/` (`.mmd` source) and export PNG.
