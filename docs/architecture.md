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

- **`Coordinator<S>`** — receives the Poise context, owns navigation state, drives controllers
- **`Controller<S>`** — fetches data via services, constructs views, mediates between view actions and service calls, returns `NavigationResult`
- **`NavigationResult`** — enum signalling the next navigation step (e.g. `Back`, `Exit`, `SettingsMain`)

### View System (`bot/views.rs`)

Views are built on three traits:

| Trait | Responsibility |
|-------|---------------|
| `View<T>` | Access to `ViewCore` (Discord I/O primitives) |
| `ResponseView` | Builds Discord components/embeds via `create_response()` |
| `InteractiveView<T>` | Exposes `run(on_action)` to controllers; extends `ResponseView` |

`RenderExt` is a blanket impl over anything that is `View + ResponseView`, providing `render()` (send-or-edit).

**`InteractiveView`** is intentionally minimal — it exposes only `handler()` and `run()`. All internal machinery lives in `InteractiveViewBase`:

- `listen_once(handler)` — collects a single interaction, calls `ViewHandler::handle()`, returns the processed action
- `register(action)` — registers an action in `ActionRegistry` and returns a `RegisteredAction` for building Discord components
- `should_acknowledge` — feature flag controlling whether interactions are auto-acknowledged
- Child view delegation via `ViewHandler::children()`

**`ViewHandler<T>`** is the extension point for implementors. Interaction-mutable state is extracted into a separate handler struct, avoiding self-referential borrows that closures stored on the view struct would create:

```
PaginationView
  ├── core: ViewCore              ← Discord I/O, ActionRegistry
  ├── base: InteractiveViewBase   ← listen_once, register, machinery
  └── handler: PaginationHandler  ← mutable state, handle(), on_timeout(), children()
```

The `impl_interactive_view!` macro generates the `InteractiveView` impl, wiring `self.base` and `self.handler` as disjoint fields. The `view_core!` macro generates the `View` impl. The `action_enum!` macro generates the `Action` impl.

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
| `FeedModel` | A content source on a platform |
| `FeedItemModel` | An individual update (chapter, episode) |
| `SubscriberModel` | A notification target (guild or DM) |
| `FeedSubscriptionModel` | Link between a feed and a subscriber |
| `ServerSettingsModel` | Per-guild configuration |
| `VoiceSessionsModel` | Voice channel session record |
| `BotMetaModel` | Key-value bot metadata |

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
      → View::new(ctx, data)         construct view with data
      → view.run(|action| { ... })   start interaction loop
          → View::render()           send Discord message
          → [listen_once loop]
              → Discord component interaction
              → ViewHandler::handle()     preprocess → domain action
              → controller callback(action)
                  → Service::save(...)    call business layer
                  → ViewCommand::Render   re-render
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
