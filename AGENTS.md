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

## Creating UI Views (Components V2)

Views use three traits:
- **`View<'a, T>`**: Core trait providing access to `ViewCore` (via `view_core!` macro)
- **`ResponseView<'a>`**: Creates Discord components/embeds via `create_response()`
- **`InteractiveView<'a, T>`**: Handles user interactions via `handle()`

### Basic View Structure

```rust
use std::time::Duration;
use crate::view_core;
use crate::bot::views::{View, ResponseView, InteractiveView, ResponseKind};
use crate::action_enum;

// 1. Define your actions
action_enum! {
    MyAction {
        #[label = "Click Me"]
        ButtonClick,
        #[label = "❮ Back"]
        Back,
    }
}

// 2. Create view struct using view_core! macro
view_core! {
    timeout = Duration::from_secs(120),
    /// Description of your view
    pub struct MyView<'a, MyAction> {
        pub counter: i32,
    }
}

// 3. Implement constructor
impl<'a> MyView<'a> {
    pub fn new(ctx: &'a Context<'a>, counter: i32) -> Self {
        Self {
            counter,
            core: Self::create_core(ctx),
        }
    }
}

// 4. Implement ResponseView to create UI components
impl<'a> ResponseView<'a> for MyView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        let components = vec![
            CreateComponent::TextDisplay(
                CreateTextDisplay::new(format!("Counter: {}", self.counter))
            ),
            CreateComponent::ActionRow(CreateActionRow::Buttons(vec![
                self.register(MyAction::ButtonClick)
                    .as_button()
                    .style(ButtonStyle::Primary),
                self.register(MyAction::Back)
                    .as_button()
                    .style(ButtonStyle::Secondary),
            ].into())),
        ];
        components.into()
    }
}

// 5. Implement InteractiveView to handle user interactions
#[async_trait::async_trait]
impl<'a> InteractiveView<'a, MyAction> for MyView<'a> {
    async fn handle(
        &mut self,
        action: &MyAction,
        _interaction: &ComponentInteraction,
    ) -> Option<MyAction> {
        match action {
            MyAction::ButtonClick => {
                self.counter += 1;
                Some(action.clone())
            }
            MyAction::Back => Some(action.clone()),
        }
    }
}
```

### Key View Components

- **Containers**: `CreateContainer` - Groups components.
- **TextDisplay**: `CreateTextDisplay` - Markdown text.
- **Sections**: `CreateSection` - Side-by-side layout.
- **Buttons**: `CreateButton` - Interactive buttons (use `self.register(action).as_button()`).
- **Select Menus**: `CreateSelectMenu` - Dropdowns (use `self.register(action).as_select(kind)`).

### View Utilities

- **`RenderExt::render()`**: Send or edit the view automatically.
- **`listen_once()`**: Wait for a single interaction (returns `Option<(Action, Interaction)>`).
- **`register(action)`**: Registers an action and returns a `RegisteredAction` with methods:
  - `.as_button()` - Convert to button
  - `.as_select(kind)` - Convert to select menu
  - `.as_select_option()` - Convert to select option

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
- **Navigation**: Use `NavigationResult` unified enum (`Exit`, `Back`, `SettingsAbout`, etc.).
- **Context**: Clone context `let ctx = *coord.context()` to avoid borrows.
- **Recursion**: Avoid it; use the loop.
- **Entry Point**: Create a legacy function that wraps the controller:
```rust
pub async fn my_command(ctx: Context<'_>) -> Result<(), Error> {
    let mut coordinator = Coordinator::new(ctx);
    let mut controller = MyController::new(&ctx);
    let _result = controller.run(&mut coordinator).await?;
    Ok(())
}
```

## Commit Conventions

Follow **Conventional Commits** specification:

```
<type>(<scope>): <subject>

<body>

<footer>
```

### Types
- `feat`: New feature
- `ui`: Changes to user interface
- `fix`: Bug fix
- `docs`: Documentation only changes
- `style`: Code style changes (formatting, semicolons, etc.)
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Adding or updating tests
- `chore`: Build process, dependencies, etc.

For user-facing commits, such as command addition, user bug fixes or UI change, insert `u_` prefix to the type, e.g., `u_feat`, `u_ui(bot)`, etc.

Keep this in mind when making user-facing commits: these commits will be detected by the CI, and used to generate changelogs for the user to see.

### Guidelines
- **Capitalize the first letter** of the subject line (unless it is strictly lowercase like a variable name)
- Use present tense ("Add feature" not "Added feature")
- Use imperative mood ("Move cursor to..." not "Moves cursor to...")
- Include motivation for change and contrast with previous behavior in body

### Examples
```
feat(voice): Add /vc stats command with contribution grid

Add voice activity statistics command that displays historical
data using GitHub-style contribution heatmaps.

- Support user and guild stats views
- Add time range selection
- Display total time, average, streak, and most active day

fix(db): Correct SQL query for daily average calculation

The subquery was not properly aliased, causing column reference
errors in SQLite.
```

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
