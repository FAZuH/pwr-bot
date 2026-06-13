# Architecture

## Layer Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Presentation Layer                       │
│     bot/ — commands (Router → CommandHandler)  +  plugins   │
├─────────────────────────────────────────────────────────────┤
│                   Application Layer                         │
│         event/  — cross-cutting event bus                   │
├─────────────────────────────────────────────────────────────┤
│                   Service Layer                             │
│               service/ — business logic                     │
├─────────────────────────────────────────────────────────────┤
│                   Domain Layer                              │
│               entity/ — models                               │
├─────────────────────────────────────────────────────────────┤
│                 Infrastructure Layer                        │
│                   repo/ — PostgreSQL via Diesel             │
└─────────────────────────────────────────────────────────────┘
```

---

## Presentation Layer (`src/bot/`)

Handles all Discord I/O. Translates Discord events into domain actions, renders UI, orchestrates navigation. Contains no business logic.

### Core Commands

Core commands are registered statically by `Cogs`:

| Module | Commands |
|--------|----------|
| `settings.rs` | `/settings` — guild feature toggle + navigation to plugin settings |
| `about.rs` | `/about` |
| `register.rs` | `/register` |
| `register_owner.rs` | `/register_owner` |
| `unregister.rs` | `/unregister` |
| `dump_db.rs` | `/dump_db` |
| `gui_test.rs` | `/gui_test` — runs all plugin-declared test steps |

### Plugin Commands

Commands from dynamic `.so` plugins are discovered at runtime via `PluginRegistry::all_commands()`. Each command resolves its plugin at dispatch time through `invocation::dispatch_plugin_command()`. No core code knows specific plugin names.

### Router → CommandHandler → View Flow

Interactive commands follow a **Router → CommandHandler → View** flow:

- **`Router`** — receives the Poise context, owns navigation state, drives handlers. Defined in `src/bot/command/mod.rs`.
- **`CommandHandler`** — trait for handler run loops.
- **`Navigation`** — enum signalling the next navigation step (e.g. `Back`, `Exit`, `SettingsMain`). Plugin settings panels use `SettingsPlugin { plugin_id }` — a dynamic string, not a hardcoded variant. Defined in `src/bot/navigation.rs`.
- **`ViewEngine`** — the event loop runner that drives the view life cycle.

### View System (`src/bot/view/mod.rs`)

The view system is built on a trait-based architecture driven by the **ViewEngine**.

| Component | Responsibility |
|-----------|---------------|
| `Action` | Trait for enums representing user actions (buttons, select menus). |
| `ViewRender<T>` | Trait defining how to translate state into Discord UI components. |
| `ViewHandler` | Trait for business logic and state mutations in response to actions. |
| `ViewEngine<T, H>` | The event loop runner that multiplexes interactions, async events, and timeouts. |
| `ViewContext<T>` | Context passed to handlers, containing the event, action, sender, and router. |

#### View Lifecycle

1. **Initialization**: `ViewEngine::new(ctx, handler, timeout, router)` is created with a handler that implements `ViewRender` and `ViewHandler`.
2. **Rendering**: The engine calls `handler.render(&mut registry)` to build the Discord message. The `ActionRegistry::register` method returns a `RegisteredAction` which provides helper methods like `.as_button()` or `.as_select()` to create Discord components.
3. **Event Loop**: `ViewEngine::run()` starts a `tokio::select!` loop listening for:
   - **Component Interactions**: Matches `custom_id` back to an `Action`.
   - **Async Events**: Dispatched via `ctx.spawn()` or `ctx.tx.send()`.
   - **Modals/Messages**: Can be integrated into the same event stream via `ViewEvent`.
   - **Timeouts**: Triggers `on_timeout()` on the handler.
4. **Command Processing**: Handlers return a `ViewCmd` to control the loop:
   - `Render`: Re-renders the view and updates the message.
   - `RenderOnce`: Renders once and exits immediately (useful for intermediate states).
   - `Continue`: Continues the loop without re-rendering.
   - `Exit`: Breaks the loop.
   - `AlreadyResponded`: Prevents auto-acknowledgment (essential for opening modals).

#### Delegation Pattern

Child views are integrated using `ctx.map(wrap, ParentAction::Child)`. This creates a `MappedViewSender` that wraps child actions into parent actions, allowing child views to be handled independently within a parent's `handle` method. This allows composition without the parent needing to know the child's internal state or action structure.

### Test Framework (`src/bot/test_framework/`)

The GUI test suite discovers test steps dynamically from loaded plugins:

- Core test steps (`/about`, `/settings`) are registered statically.
- Plugin test steps are discovered from `PluginRegistry::all_test_steps()` at runtime.
- Each test step dispatches via `dispatch_plugin_command()` — the same path used for production commands.

---

## Application Layer (`src/event/`)

Cross-cutting concerns that don't belong to any single feature. Glues layers together without containing business logic.

### Event System (`event/`)

Type-safe pub/sub via `EventBus`. Publishers and subscribers are decoupled — neither knows about each other.

| Event | Published by | Consumed by |
|-------|-------------|-------------|
| `VoiceStateEvent` (named `"voice_state"`) | `BotEventHandler` | Plugins subscribed via `event_handlers()` |

The event bus supports both typed events (via `Event` trait) and named string-based events. Named events are the primary mechanism for plugin communication — any plugin can subscribe to named events by declaring `EventHandlerSpec` entries.

---

## Service Layer (`src/service/`)

The only layer that enforces business rules. Handlers call services; services orchestrate repositories and platforms. Nothing above this layer touches data directly.

| Service (trait) | Responsibility |
|---------|---------------|
| `SettingsProvider` | Server configuration management — reads/writes `server_settings` JSONB via the dynamic `plugin_settings` HashMap |
| `InternalOps` | Bot metadata (`bot_version`), internal operations |

---

## Domain Layer (`src/entity.rs`)

Plain domain objects. Entities have no database concerns beyond Diesel model derives.

### Entities

| Entity | Description |
|--------|-------------|
| `ServerSettingsEntity` | Per-guild configuration — stores all plugin settings in a `#[serde(flatten)] HashMap<String, Value>` |
| `BotMetaEntity` | Key-value bot metadata |

### Plugin Settings Storage

`ServerSettings` uses `#[serde(flatten)]` so existing JSONB documents like:
```json
{"feeds": {"enabled": true}, "voice": {"enabled": false}}
```
deserialize directly into a `HashMap<String, Value>` without migration. Helper methods `is_enabled(id)` and `set_enabled(id, val)` abstract the access pattern.

---

## Infrastructure Layer (`src/repo/`)

Data access. Repositories depend on domain entities, not the other way around. Owns all Diesel query logic, the connection pool, and migrations.

A factory trait `Repos` defines the repo access interface:

```rust
pub trait Repos: Send + Sync {
    fn server_settings(&self) -> Box<dyn ServerSettingsRepository + Send + Sync>;
    fn bot_meta(&self) -> Box<dyn BotMetaRepository + Send + Sync>;
}
```

The concrete `PgRepos` struct implements the factory with:

```rust
pub struct PgRepos {
    pub server_settings: PgServerSettingsRepo,
    pub bot_meta: PgBotMetaRepo,
    pool: DbPool,
}
```

Plugin crates manage their own database connections via `deadpool-postgres` pools, reading `DB_URL` from the environment. They are **not** part of the core repo layer.

---

## Plugin System (`crates/pwr_bot_sdk/`, `src/bot/plugin/`)

Plugins are loaded as dynamic shared libraries (`.so`) at startup from `PLUGIN_DIR` (default: `./plugins/`). They communicate with the host through a stable C ABI defined in the `pwr_bot_sdk` crate.

### Architecture

```
Discord interaction
  → Poise routes to command
  → PluginRegistry::all_commands() resolves plugin
  → invocation::dispatch_plugin_command()
    → [FFI]   InvokeRequest → VTable::invoke() → host callbacks
```

### PluginRegistry Discovery

The `PluginRegistry` provides dynamic discovery methods that iterate all loaded plugins:

| Method | Returns | Used by |
|--------|---------|---------|
| `all_commands()` | `Vec<Command<Data, Error>>` | Poise framework registration |
| `all_settings_panels()` | `Vec<(plugin_name, id, SettingsPanelSpec)>` | `/settings` UI |
| `all_event_handlers()` | `Vec<(LoadedPlugin, EventHandlerSpec)>` | Event bus subscription |
| `all_test_steps()` | `Vec<(plugin_name, step_name, TestStepSpec)>` | `/gui_test` command |
| `all_tasks()` | `Vec<(plugin_name, TaskSpec)>` | Background task spawning |

### HostCtx Abstraction

The [`HostCtx`](src/bot/host_ctx.rs) trait decouples handler logic from poise internals:

| Implementation | Backing | Use case |
|----------------|---------|----------|
| `PoiseHostCtx` | `Context<'_>` (poise) | Core commands |
| `FfiHostCtx` | `HostCallbacks` (FFI) | Plugin commands — wraps `PoiseHostCtx` in a handle-based registry |

All plugin commands (including events and tasks) go through `FfiHostCtx`. The `host_registry` maps opaque `u64` handles back to `PoiseHostCtx` instances for callback dispatch.

### SDK Crate (`crates/pwr_bot_sdk/`)

| Module | Contents |
|--------|----------|
| `abi.rs` | `PluginVTable`, `InvokeRequest`/`InvokeResponse`, `HostCallbacks` — all `#[repr(C)]` |
| `host.rs` | `PluginHost` — safe wrapper around FFI callbacks |
| `plugin.rs` | `BotPlugin` trait, metadata specs (`CommandSpec`, `TestStepSpec`, `SettingsPanelSpec`, `EventHandlerSpec`, `TaskSpec`) |
| `macros.rs` | `export_plugin!` — generates `extern "C"` entry point with `OnceLock` lazy initialization |

### ABI Contract

```rust
#[repr(C)]
pub struct PluginVTable {
    pub api_version: u32,
    pub metadata:     unsafe extern "C" fn() -> *mut c_char,  // JSON PluginMetadata
    pub invoke:       unsafe extern "C" fn(req, resp),
    pub init:         Option<unsafe extern "C" fn(req, resp)>,
    pub shutdown:     Option<unsafe extern "C" fn() -> bool>,
    pub on_event:     Option<unsafe extern "C" fn(event_name, payload, callbacks, ctx) -> bool>,
    pub free_string:  Option<unsafe extern "C" fn(*mut c_char)>,
}
```

The host calls `metadata()` at load time to discover slash commands, settings panels, event handlers, tasks, and test steps — all encoded as a JSON `PluginMetadata` struct. When a user invokes a plugin command, the host serializes the arguments into `InvokeRequest`, calls `invoke()`, and processes the `InvokeResponse`.

### Plugin Lifecycle

1. **Load**: `loader::load_plugins("plugins/")` — `dlopen` each `.so`, lookup `pwr_bot_plugin_entry`, validate `api_version`
2. **Register**: `registry::PluginRegistry::register()` — parse `PluginMetadata` JSON, construct poise `Command` objects, register event handlers
3. **Init**: `invocation::dispatch_init_ffi()` — calls `vtable.init()` for each plugin (DB pool setup, etc.)
4. **Subscribe**: `register_ffi_event_handlers()` — subscribes to named events on the event bus
5. **Tasks**: `dispatch_tasks_ffi()` — spawns tokio intervals for each `TaskSpec`
6. **Invoke**: `dispatch()` — serialize args, call FFI `invoke()`, deserialize `ResponsePayload`, send to Discord
7. **Teardown**: Plugins are never unloaded — library handles are leaked intentionally so vtable pointers remain valid

### Writing a Plugin

```rust
use pwr_bot_sdk::export_plugin;
use pwr_bot_sdk::{BotPlugin, CommandSpec, ResponsePayload, PluginHost, TestStepSpec};

struct MyPlugin;

#[async_trait]
impl BotPlugin for MyPlugin {
    fn name(&self) -> &'static str { "my-plugin" }
    fn description(&self) -> &'static str { "Example plugin" }
    fn version(&self) -> &'static str { "0.1.0" }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![CommandSpec::new("hello", "Says hello")]
    }

    fn settings_panels(&self) -> Vec<SettingsPanelSpec> {
        vec![SettingsPanelSpec::new("my-plugin", "My Plugin")]
    }

    fn test_steps(&self) -> Vec<TestStepSpec> {
        vec![TestStepSpec::new(
            "hello", "Says hello", "hello", serde_json::json!({}),
        )]
    }

    async fn invoke(&self, cmd: &str, _args: serde_json::Value, _host: &PluginHost) -> Result<ResponsePayload, String> {
        match cmd {
            "hello" => Ok(ResponsePayload {
                content: Some("Hello from plugin!".into()),
                ephemeral: false,
                components_json: None,
                embed_json: None,
            }),
            _ => Err("Unknown command".into()),
        }
    }
}

export_plugin!(MyPlugin, MyPlugin);
```

### Plugin Database Access

Each plugin manages its own `deadpool-postgres` connection pool using `DB_URL` from the environment. The pool is created during `init()`:

```rust
async fn init(&self, _host: &PluginHost) -> Result<(), String> {
    let mut config = deadpool_postgres::Config::new();
    config.url = Some(std::env::var("DB_URL").map_err(|_| "DB_URL not set")?);
    let pool = config.create_pool(Some(Runtime::Tokio1), NoTls)
        .map_err(|e| e.to_string())?;
    *self.pool.lock().await = Some(pool);
    Ok(())
}
```

---

## Event Lifecycles

### User Interaction

```
Discord interaction
  → Poise routes to command entry function
  → Router::new(ctx)
  → Router::run(initial)             starts navigation loop
      → CommandHandler::run(router)
          → Service::fetch(...)          fetch required data
          → ViewHandler::new(...)        construct handler state
          → ViewEngine::run(...)         start event loop (tokio::select!)
              → ViewRender::render()     build Discord components
              → [Event Loop]
                  → Discord interaction / Async event / Modal
                  → ViewHandler::handle(ctx)  process action → state mutation
                  → ViewCmd::Render           re-render view
              → ViewCmd::Exit
          → router.navigate(next)      signal next navigation step
      → Router routes to next CommandHandler or exits
```

### Plugin Command

```
Discord interaction
  → Poise routes to registry command handler
  → PluginRegistry::lookup(command_name)
  → invocation::dispatch_plugin_command()
    → dispatch()                         FFI path
      → serialize args to JSON
      → FfiHostCtx (handle + callbacks)
      → VTable::invoke(request, response)
      → parse ResponsePayload
      → send reply via host callbacks
```

### Voice State Event

```
Discord gateway event
  → BotEventHandler::dispatch()
  → EventBus::publish_named("voice_state", payload)
  → All plugins subscribed to "voice_state"
  → Each plugin's VTable::on_event()
```

---

## Design Patterns Summary

| Pattern | Where | Purpose |
|---------|-------|---------|
| Router → CommandHandler | Presentation | Navigation loop driving per-domain handlers |
| ViewHandler | Presentation | Separates interaction state from view machinery |
| Repository (factory) | Infrastructure | `Repos` trait with `PgRepos` concrete impl |
| Event Bus | Application | Decoupled pub/sub communication |
| Service | Application | Business logic via trait objects |
| Update (TEA) | Application | Pure state mutations separated from side effects |
| Plugin via FFI | Cross-cutting | Dynamic `.so` loading with stable C ABI |
