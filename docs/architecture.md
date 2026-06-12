# Architecture

## Layer Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Presentation Layer                       │
│          bot/ — commands (Router → CommandHandler)          │
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
│                   repo/ — PostgreSQL via Diesel             │
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

### Router → CommandHandler → View Flow

Interactive commands follow a **Router → CommandHandler → View** flow:

- **`Router`** — receives the Poise context, owns navigation state, drives handlers. Defined in `src/bot/command/mod.rs`.
- **`CommandHandler`** — trait for handler run loops. Each domain has a concrete handler (e.g. `FeedListHandler`, `VoiceStatsHandler`).
- **`Navigation`** — enum signalling the next navigation step (e.g. `Back`, `Exit`, `SettingsMain`). Defined in `src/bot/navigation.rs`.
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

The only layer that enforces business rules. Handlers call services; services orchestrate repositories and platforms. Nothing above this layer touches data directly.

| Service (trait) | Responsibility |
|---------|---------------|
| `FeedSubscriptionProvider` | Feed subscription lifecycle — create, delete, list, validate |
| `VoiceTracker` | Voice session lifecycle — start, stop, query stats |
| `SettingsProvider` | Server configuration management |
| `InternalOps` | Bot metadata and internal operations |

---

## Domain Layer (`src/feed/`, `src/entity.rs`)

Plain domain objects and platform abstractions. Entities have no database concerns beyond `FromRow` (an acceptable tradeoff). Platform implementations depend on domain types, not the other way around.

### Entities (`src/entity.rs`)

| Entity | Description |
|--------|-------------|
| `FeedEntity` | A content source on a platform |
| `FeedItemEntity` | An individual update (chapter, episode) |
| `SubscriberEntity` | A notification target (guild or DM) |
| `FeedSubscriptionEntity` | Link between a feed and a subscriber |
| `ServerSettingsEntity` | Per-guild configuration, includes nested `WelcomeSettings`, `FeedsSettings`, `VoiceSettings` |
| `VoiceSessionsEntity` | Voice channel session record |
| `BotMetaEntity` | Key-value bot metadata |
| `DbVoiceSession` | Raw voice session for persistence |
| `VoiceLeaderboardEntry` / `VoiceLeaderboardRow` | Leaderboard query results |

### Platforms (`feed/`)

Implements the **Strategy pattern** — `FeedSubscriptionService` depends on the `Platform` trait, not concrete implementations.

| Platform | API |
|----------|-----|
| `MangaDexPlatform` | MangaDex |
| `AniListPlatform` | AniList |
| `ComickPlatform` | Comick |

---

## Infrastructure Layer (`src/repo/`)

Data access. Repositories depend on domain entities, not the other way around. Owns all Diesel query logic, the connection pool, and migrations.

A factory trait `Repos` defines the repo access interface. The concrete `PgRepos` struct holds per-table `Pg*Repo` handles and implements the factory:

```rust
pub trait Repos: Send + Sync {
    fn feed(&self) -> Box<dyn FeedRepository + Send + Sync>;
    fn feed_item(&self) -> Box<dyn FeedItemRepository + Send + Sync>;
    fn subscriber(&self) -> Box<dyn SubscriberRepository + Send + Sync>;
    fn feed_subscription(&self) -> Box<dyn FeedSubscriptionRepository + Send + Sync>;
    fn server_settings(&self) -> Box<dyn ServerSettingsRepository + Send + Sync>;
    fn voice_sessions(&self) -> Box<dyn VoiceSessionsRepository + Send + Sync>;
    fn bot_meta(&self) -> Box<dyn BotMetaRepository + Send + Sync>;
}

pub struct PgRepos {
    pub feed: PgFeedRepo,
    pub feed_item: PgFeedItemRepo,
    pub subscriber: PgSubscriberRepo,
    pub feed_subscription: PgFeedSubscriptionRepo,
    pub server_settings: PgServerSettingsRepo,
    pub voice_sessions: PgVoiceSessionsRepo,
    pub bot_meta: PgBotMetaRepo,
    pool: DbPool,
}
```

Each table struct (`Pg*Repo`) implements a `CrudTable<T, ID>` trait alongside domain-specific repository traits.

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
  → PgRepos                          persist to PostgreSQL
```

---

## Design Patterns Summary

| Pattern | Where | Purpose |
|---------|-------|---------|
| Router → CommandHandler | Presentation | Navigation loop driving per-domain handlers |
| ViewHandler | Presentation | Separates interaction state from view machinery |
| Strategy | Domain | Swappable platform implementations |
| Repository (factory) | Infrastructure | `Repos` trait with `PgRepos` concrete impl |
| Event Bus | Application | Decoupled pub/sub communication |
| Service | Application | Business logic via trait objects (`SettingsProvider`, `FeedSubscriptionProvider`, etc.) |
| Update (TEA) | Application | Pure state mutations separated from side effects |
