# AGENTS.md

This document provides guidelines for AI agents working on the pwr-bot Rust codebase.

## Project Overview

- **Type**: Discord bot with feed subscriptions and voice channel tracking
- **Language**: Rust (Edition 2024)
- **Database**: SQLite with SQLx
- **Framework**: Serenity + Poise for Discord integration
- **Async Runtime**: Tokio

## Development Commands

**Prefer using `./dev.sh`** for common tasks. Use manual commands only for granular control.

```bash
# Using dev.sh (recommended)
./dev.sh format      # Format code
./dev.sh lint        # Run linter (with --fix)
./dev.sh test        # Run tests
./dev.sh build       # Build Docker image
./dev.sh all         # Run format, lint, test, build

# Run multiple commands
./dev.sh format lint test
```

Use manual commands only when you need granular control:

```bash
# Build the project
cargo build
cargo build --all-features --all-targets

# Run the bot (requires .env file)
cargo run
cargo run --release

# Run all tests
cargo test --all-features
cargo test --all-features --no-fail-fast

# Run a specific test
cargo test test_name
cargo test --test voice_tracking_service_test
cargo test voice_tracking

# Format code
cargo +nightly fmt --all
cargo +nightly fmt --all -- --check

# Run Clippy
cargo clippy --all-features --all-targets
cargo clippy --all-features --all-targets -- -D warnings
```

### Test Environment

- Tests require `SQLX_OFFLINE=true` for CI (already configured in .github/workflows)
- Tests use SQLite in-memory or file-based databases with automatic cleanup

## Code Style Guidelines

### Import Organization

Follow `rustfmt.toml` configuration with `group_imports = "StdExternalCrate"`:

```rust
// 1. Standard library imports
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

// 2. External crate imports
use anyhow::Result;
use async_trait::async_trait;
use log::debug;
use log::info;
use poise::Framework;
use sqlx::SqlitePool;

// 3. Crate-local imports
use crate::bot::commands::Cog;
use crate::config::Config;
use crate::database::Database;
```

With `imports_granularity = "Item"`, each item must be imported separately (no `use crate::module::*;`).

### Formatting

- **Max line length**: Default (100 chars recommended)
- **Indentation**: 4 spaces (standard Rust)
- **Trailing commas**: Required in multi-line structs/enums

### Naming Conventions

- **Structs/Enums/Traits**: `PascalCase` (e.g., `FeedSubscriptionService`, `VoiceStateEvent`)
- **Functions/Methods**: `snake_case` (e.g., `get_or_create_feed`, `is_enabled`)
- **Variables**: `snake_case` (e.g., `feed_item`, `guild_id`)
- **Constants**: `SCREAMING_SNAKE_CASE` (e.g., `MAX_RETRY_ATTEMPTS`)
- **Type Parameters**: Single uppercase letter (e.g., `T`, `E`)
- **Modules**: `snake_case` (e.g., `voice_tracking_service.rs`)
- **Error types**: Suffix with `Error` (e.g., `DatabaseError`, `FeedError`)

### Error Handling

1. **Use `anyhow` for application errors**:
   - Return `anyhow::Result<T>` from main functions
   - Use `?` operator for error propagation

2. **Use `thiserror` for custom error types**:
   ```rust
   #[derive(Debug, thiserror::Error)]
   #[non_exhaustive]
   pub enum AppError {
       #[error("Missing config \"{config}\"")]
       MissingConfig { config: String },
   }
   ```

3. **Module-level error files**: Each module should have an `error.rs` file

4. **Error variants should be descriptive** and use structured data:
   ```rust
   #[error("Error in app configuration: {msg}")]
   ConfigurationError { msg: String },
   ```

### Types and Generics

- Prefer explicit types over `impl Trait` in function signatures
- Use `Arc<T>` for shared state across async boundaries
- Use `async_trait` for trait methods that need to be async
- Use type aliases for complex types: `type Error = Box<dyn std::error::Error + Send + Sync>`;

### Async Patterns

- Use `tokio::spawn` for concurrent tasks
- Prefer `&self` over `&mut self` when possible (interior mutability pattern)
- Use `tokio::sync::Mutex` for async-aware locking, `std::sync::Mutex` for sync contexts
- Spawn blocking operations: `tokio::task::spawn_blocking`

### Documentation

- Add doc comments to all public items (`///`)
- Include examples in doc comments for complex functions
- Document panics, errors, and safety requirements
- Use module-level documentation (`//!`) at top of module files

### Testing

- Use descriptive test names: `test_<function_name>_<scenario>`
- Use `#[tokio::test]` for async tests
- Create test utilities in `tests/common/` module
- Clean up resources in tests using teardown functions

### SQLx and Database

- Use SQLx offline mode for CI: `SQLX_OFFLINE=true`
- Run `cargo sqlx prepare` to update query metadata
- Use parameterized queries only (never string interpolation)
- Migrations are in `migrations/` directory

### Logging

- Use the `log` crate macros: `info!`, `debug!`, `warn!`, `error!`
- Include context in log messages
- Use `log::log!(log::Level::Info, ...)` for dynamic levels
- Log initialization timing using `std::time::Instant`

### Project Structure

```
src/
├── main.rs              # Application entry point
├── lib.rs               # Library exports
├── config.rs            # Configuration handling
├── error.rs             # Top-level errors
├── logging.rs           # Logging setup
├── bot/                 # Discord bot module
│   ├── mod.rs
│   ├── commands/        # Bot commands
│   ├── views/           # UI components
│   └── error.rs
├── database/            # Database layer
│   ├── mod.rs
│   ├── model/           # Data models
│   ├── table/           # Table operations
│   └── error.rs
├── service/             # Business logic
│   ├── mod.rs
│   ├── feed_subscription_service.rs
│   └── voice_tracking_service.rs
├── feed/                # Feed platform integrations
├── subscriber/          # Event subscribers
└── task/                # Background tasks
```

## Adding Commands

Commands are organized using the **Cog pattern**. Each command module should:

1. **Create a Cog struct** that implements the `Cog` trait
2. **Implement commands** as associated functions with `#[poise::command]` attribute
3. **Return commands** in the `commands()` method
4. **Register in `Cogs`** by adding to the chain in `src/bot/commands/mod.rs`

### Example Command Module

```rust
use poise::Command;

use crate::bot::commands::Cog;
use crate::bot::commands::Context;
use crate::bot::commands::Error;

pub struct MyCommandCog;

impl MyCommandCog {
    #[poise::command(slash_command)]
    pub async fn mycommand(ctx: Context<'_>) -> Result<(), Error> {
        ctx.defer().await?;
        ctx.say("Hello!").await?;
        Ok(())
    }
}

impl Cog for MyCommandCog {
    fn commands(&self) -> Vec<Command<crate::bot::Data, Error>> {
        vec![Self::mycommand()]
    }
}
```

### Registering the Command

Add to `src/bot/commands/mod.rs`:

```rust
pub use mycommand::MyCommandCog;

impl Cog for Cogs {
    fn commands(&self) -> Vec<Command<Data, Error>> {
        let mycommand_cog = MyCommandCog;
        // ... other cogs
        
        feeds_cog
            .commands()
            // ... other chains
            .chain(mycommand_cog.commands())
            .collect()
    }
}
```

## Creating UI Views (Components V2)

Views use Discord's **Components V2** system for rich UI. The trait hierarchy is:

1. **ViewProvider** - Creates UI components
2. **ResponseComponentView** - Sends views as replies/messages (blanket impl of ViewProvider)
3. **StatefulView** - Manages view context with send/edit capability
4. **InteractableComponentView** - Handles component interactions (extends StatefulView)

### Basic Static View

For simple views that don't need interaction or state management:

```rust
use crate::bot::views::ResponseComponentView;

pub struct MyView {
    data: String,
}

impl MyView {
    pub fn new(data: String) -> Self {
        Self { data }
    }
}

impl ResponseComponentView for MyView {
    fn create_components<'a>(&self) -> Vec<CreateComponent<'a>> {
        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                format!("## {}", self.data)
            )),
        ]));
        
        vec![container]
    }
}
```

### Interactive View (with state and interactions)

For views that handle interactions and manage state, use the `stateful_view!` macro to automatically implement `StatefulView`:

```rust
use std::time::Duration;
use crate::stateful_view;
use crate::bot::views::Action;
use crate::bot::views::InteractableComponentView;
use crate::custom_id_enum;

// Define actions using the macro
custom_id_enum!(MyAction {
    Confirm,
    Cancel,
});

// Define the view with automatic StatefulView implementation
stateful_view! {
    timeout = Duration::from_secs(120),
    pub struct InteractiveView<'a> {
        confirmed: bool,
    }
}

impl ResponseComponentView for InteractiveView<'_> {
    fn create_components<'a>(&self) -> Vec<CreateComponent<'a>> {
        // Create components based on state
        vec![/* ... */]
    }
}

#[async_trait::async_trait]
impl<'a> InteractableComponentView<'a, MyAction> for InteractiveView<'a> {
    async fn handle(&mut self, interaction: &ComponentInteraction) -> Option<MyAction> {
        let action = MyAction::from_str(&interaction.data.custom_id).ok()?;
        
        match action {
            MyAction::Confirm => {
                self.confirmed = true;
                Some(action)
            }
            MyAction::Cancel => Some(action),
        }
    }
}
```

The macro generates:
- A struct with an added `ctx: ViewContext<'a, ()>` field
- A `new(ctx: &Context<'a>, ...)` constructor that initializes the context with the specified timeout
- Implementation of `StatefulView<'a, ()>` with `view_context()` and `view_context_mut()` methods

#### Manual StatefulView Implementation

If you need custom behavior or a different data type, implement `StatefulView` manually:

```rust
use crate::bot::views::StatefulView;
use crate::bot::views::ViewContext;

pub struct InteractiveView<'a> {
    confirmed: bool,
    ctx: ViewContext<'a, ()>,
}

impl<'a> InteractiveView<'a> {
    pub fn new(ctx: &'a Context<'a>) -> Self {
        Self {
            confirmed: false,
            ctx: ViewContext::new(ctx, Duration::from_secs(120)),
        }
    }
}

#[async_trait::async_trait]
impl<'a> StatefulView<'a> for InteractiveView<'a> {
    fn view_context(&self) -> &ViewContext<'a> {
        &self.ctx
    }

    fn view_context_mut(&mut self) -> &mut ViewContext<'a> {
        &mut self.ctx
    }
}
```

### Using Views in Commands

```rust
impl MyCommandCog {
    #[poise::command(slash_command)]
    pub async fn show(ctx: Context<'_>) -> Result<(), Error> {
        ctx.defer().await?;
        
        let view = MyView::new("Hello World".to_string());
        ctx.send(view.create_reply()).await?;
        
        Ok(())
    }
}
```

### Key View Components

- **Containers**: `CreateContainer` - Groups components together
- **TextDisplay**: `CreateTextDisplay` - Markdown text content
- **Sections**: `CreateSection` - Side-by-side layout with thumbnail/accessory
- **Buttons**: `CreateButton` - Interactive buttons (use `Cow::Owned(vec![...])` for action rows)
- **Select Menus**: `CreateSelectMenu` - Dropdown selections

### CI/CD

- GitHub Actions runs format check, build, clippy, and tests
- Uses nightly rustfmt for unstable features
- Uses stable clippy for linting
- All checks must pass before merging

## Design Patterns

### Controller Pattern

Use the Controller pattern for interactive command flows:

1. **Controller trait**: All interactive flows implement `Controller<O>`
2. **Coordinator**: Use `Coordinator::run()` to chain controllers without recursion
3. **Output enums**: Controllers return enums indicating navigation intent

```rust
// Controller implementation
pub struct SettingsController<'a> { ctx: &'a Context<'a> }

impl<'a> Controller<SettingsResult> for SettingsController<'a> {
    async fn run(&mut self) -> Result<SettingsResult, Error> {
        // Interactive flow logic
        Ok(SettingsResult::Exit)
    }
}

// Chaining with Coordinator
Coordinator::run(
    SettingsMainController::new(&ctx),
    |result| match result {
        MainResult::NavigateToFeeds => Some(SettingsFeedController::new(&ctx)),
        MainResult::Exit => None,
    }
).await?;
```

### Type Design

- **Prefer generic types** over associated types when both are possible
  - Generics offer more flexibility and composability
  - Associated types should only be used when the type is truly inherent to the trait
  - Example: Use `Controller<O>` instead of `trait Controller { type Output; }`

### UI Guidelines

- **Do not use emoji** in Discord UI components
  - Discord emoji render inconsistently across platforms
  - Use Unicode glyphs or plain text labels instead
  - Example: Use "< Back" instead of emoji arrows
  - For icons, use a consistent set of Unicode characters (e.g., mathematical symbols)

### Navigation Patterns

- **Flat navigation**: Use a Coordinator to manage controller flow
- **Back navigation**: Controllers return a result indicating where to go next
- **State persistence**: Pass shared state through controller constructors
- **No recursion**: Always use the Coordinator pattern to prevent stack overflow

## Documentation

### Architecture Diagrams

Update diagrams in `docs/diagrams/` when making architectural changes:

- `architecture-view-uml.mmd` - Class diagrams
- `architecture-view-flow.mmd` - Flow diagrams

**Guidelines:**
- Keep source in Mermaid format (`.mmd`)
- Export PNG for viewing
- Mark unimplemented features in dedicated section
