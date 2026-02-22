# Architecture

## Layer Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Presentation Layer                       │
│              bot/ — commands, views, controllers            │
├─────────────────────────────────────────────────────────────┤
│                   Application Layer                         │
│         event/  subscriber/  task/  — cross-cutting         │
├─────────────────────────────────────────────────────────────┤
│                    Service Layer                            │
│                service/ — business logic                    │
├─────────────────────────────────────────────────────────────┤
│                    Domain Layer                             │
│           feed/  entity/ — models and platforms             │
├─────────────────────────────────────────────────────────────┤
│                 Infrastructure Layer                        │
│              repository/ — SQLite via SQLx                  │
└─────────────────────────────────────────────────────────────┘
```

---

## Presentation Layer (`src/bot/`)

Handles all Discord I/O. Translates Discord events into domain actions, renders UI, orchestrates navigation. Contains no business logic.

### Commands (`bot/commands/`)

Commands are organized by domain. Each top-level module is a command group; subcommands live in a subdirectory of the same name.

| Module | Commands |
|--------|----------|
| `feed.rs` | `/feed` group — `list`, `subscribe`, `unsubscribe`, `settings` |
| `voice.rs` | `/vc` group — `leaderboard`, `stats`, `settings` |
| `settings.rs` | `/settings` group — `feeds`, `voice` |
| `about.rs` | `/about` |
| `register.rs` | `/register` |
| `register_owner.rs` | `/register_owner` |
| `unregister.rs` | `/unregister` |
| `dump_db.rs` | `/dump_db` |

### MVC-C Pattern

Interactive commands follow a **Coordinator → Controller → View** flow:

- **`Coordinator<S>`** — receives the Poise context, owns navigation state, drives controllers.
- **`Controller<S>`** — fetches data via services, constructs views, mediates between view actions and service calls, returns `NavigationResult`.
- **`NavigationResult`** — enum signalling the next navigation step (e.g. `Back`, `Exit`, `SettingsMain`).
- **`ViewEngine`** — the event loop runner that drives the view life cycle.

### View System (`src/bot/views.rs`)

The view system is built on a trait-based architecture driven by the **ViewEngine**.

| Component | Responsibility |
|-----------|---------------|
| `Action` | Trait for enums representing user actions (buttons, select menus). |
| `ViewRender<T>` | Trait defining how to translate state into Discord UI components. |
| `ViewHandler<T>` | Trait for business logic and state mutations in response to actions. |
| `ViewEngine<T, H>` | The event loop runner that multiplexes interactions, async events, and timeouts. |
| `ViewContext<T>` | Context passed to handlers, allowing for async task spawning and child view mapping. |

#### View Lifecycle

1. **Initialization**: `ViewEngine::new(ctx, handler, timeout)` is created with a handler that implements both `ViewRender` and `ViewHandler`.
2. **Rendering**: The engine calls `handler.render(&mut registry)` to build the Discord message. The `ActionRegistry` maps Discord `custom_id`s to `Action` variants.
3. **Event Loop**: `ViewEngine::run()` starts a `tokio::select!` loop listening for:
   - **Component Interactions**: Matches `custom_id` back to an `Action`.
   - **Async Events**: Dispatched via `ctx.spawn()` or `ctx.tx.send()`.
   - **Modals/Messages**: Can be integrated into the same event stream.
   - **Timeouts**: Triggers `on_timeout()` on the handler.
4. **Command Processing**: Handlers return a `ViewCommand` to control the loop:
   - `Render`: Re-renders the view and updates the message.
   - `Continue`: Continues the loop without re-rendering.
   - `Exit`: Breaks the loop.
   - `AlreadyResponded`: Prevents auto-acknowledgment (essential for opening modals).

#### Delegation Pattern

Child views are integrated using `ctx.map(ParentAction::Child)`. This creates a sub-context that wraps child actions into parent actions, allowing child views to be handled independently within a parent's `handle` method. This allows composition without the parent needing to know the child's internal state or action structure.

---

## Application Layer (`src/event/`, `src/subscriber/`, `src/task/`)

Cross-cutting concerns that don't belong to any single feature. Glues layers together without containing business logic.

### Event System (`event/`)

Type-safe pub/sub via `EventBus`. Publishers and subscribers are decoupled — neither knows about each other.

| Event | Published by | Consumed by |
|-------|-------------|-------------|
| `FeedUpdateEvent` | `SeriesFeedPublisher` | `DiscordGuildSubscriber`, `DiscordDmSubscriber` |
| `VoiceStateEvent` | `BotEventHandler` | `VoiceStateSubscriber` |

### Subscribers (`subscriber/`)

React to application events, call services, send Discord messages.

| Subscriber | Reacts to |
|-----------|----------|
| `DiscordGuildSubscriber` | `FeedUpdateEvent` → sends to guild channel |
| `DiscordDmSubscriber` | `FeedUpdateEvent` → sends to DM |
| `VoiceStateSubscriber` | `VoiceStateEvent` → tracks session lifecycle |

### Background Tasks (`task/`)

| Task | Responsibility |
|------|---------------|
| `SeriesFeedPublisher` | Polls feed platforms on a schedule, publishes `FeedUpdateEvent` |
| `VoiceHeartbeatManager` | Crash recovery for active voice sessions |

---

## Service Layer (`src/service/`)

The only layer that enforces business rules. Controllers call services; services orchestrate repositories and platforms. Nothing above this layer touches data directly.

| Service | Responsibility |
|---------|---------------|
| `FeedSubscriptionService` | Feed subscription lifecycle — create, delete, list, validate |
| `VoiceTrackingService` | Voice session lifecycle — start, stop, query stats |
| `SettingsService` | Server configuration management |
| `InternalService` | Bot metadata and internal operations |

---

## Domain Layer (`src/feed/`, `src/model.rs`)

Plain domain objects and platform abstractions. Entities have no database concerns beyond `FromRow` (an acceptable tradeoff). Platform implementations depend on domain types, not the other way around.

### Entities (`model.rs`)

> **Note:** Will be renamed to `entity.rs` in a future refactor.

| Entity | Description |
|--------|-------------|
| `FeedEntity` | A content source on a platform |
| `FeedItemEntity` | An individual update (chapter, episode) |
| `SubscriberEntity` | A notification target (guild or DM) |
| `FeedSubscriptionEntity` | Link between a feed and a subscriber |
| `ServerSettingsEntity` | Per-guild configuration |
| `VoiceSessionsEntity` | Voice channel session record |
| `BotMetaEntity` | Key-value bot metadata |

### Platforms (`feed/`)

Implements the **Strategy pattern** — `FeedSubscriptionService` depends on the `Platform` trait, not concrete implementations.

| Platform | API |
|----------|-----|
| `MangaDexPlatform` | MangaDex |
| `AniListPlatform` | AniList |
| `ComickPlatform` | Comick |

---

## Infrastructure Layer (`src/repository/`)

Data access. Repositories depend on domain entities, not the other way around. Owns all SQLx query logic, the connection pool, and migrations.

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
}
```

Each table struct is responsible for CRUD operations on its domain area.

---

## Event Lifecycles

### User Interaction

```
Discord interaction
  → Poise routes to command entry function
  → Coordinator::new(ctx)
  → Controller::run(coordinator)
      → Service::fetch(...)          fetch required data
      → ViewHandler::new(...)        construct handler state
      → ViewEngine::run(...)         start event loop (tokio::select!)
          → ViewRender::render()     build Discord components
          → [Event Loop]
              → Discord interaction / Async event / Modal
              → ViewHandler::handle()     process action → state mutation
              → ViewCommand::Render       re-render view
          → ViewCommand::Exit
      → NavigationResult
  → Coordinator routes to next Controller or exits
```

### Background Feed Update

```
SeriesFeedPublisher (scheduled)
  → Platform::fetch_latest()         poll external API
  → FeedSubscriptionService          validate, find subscribers
  → EventBus::publish(FeedUpdateEvent)
  → DiscordGuildSubscriber / DiscordDmSubscriber
  → Send Discord message via Serenity HTTP
```

### Voice State Change

```
Discord gateway event
  → BotEventHandler::dispatch()
  → EventBus::publish(VoiceStateEvent)
  → VoiceStateSubscriber
  → VoiceTrackingService             update session state
  → Repository                       persist to SQLite
```

---

## Design Patterns Summary

| Pattern | Where | Purpose |
|---------|-------|---------|
| MVC-C | Presentation | Coordinator → Controller → View navigation |
| ViewHandler | Presentation | Separates interaction state from view machinery |
| Strategy | Domain | Swappable platform implementations |
| Repository | Infrastructure | Database abstraction |
| Event Bus | Application | Decoupled pub/sub communication |
| Service | Service | Business logic encapsulation |
