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

### Router → CommandHandler → Host Flow

Interactive commands follow a **Router → CommandHandler → Host** flow:

- **`Router`** — receives the Poise context, owns navigation state, drives handlers. Its session loop keeps the frames open on the message: each target that runs becomes the newest frame, and `Back` closes the newest frame and re-runs the one beneath it, morphing the same message. Defined in `src/bot/command/mod.rs`.
- **`CommandHandler`** — trait for handler run loops. Each domain has a concrete handler (e.g. `FeedListHandler`, `VoiceStatsHandler`).
- **`Navigation`** — enum signalling the next navigation step (e.g. `Back`, `Exit`, `SettingsMain`). `SettingsMain` hands the live message to the settings plugin's hub view, and a `Back` over the last open frame dismisses the message. Both terminal steps live in `src/bot/command/session_exit.rs`. Defined in `src/bot/navigation.rs`.
- **`Host`** — the TEA event loop that runs one interactive view. Defined in `src/bot/gui/rt.rs`.

### Interactive Views (TEA — `src/update/` cores + `src/bot/gui/` shell)

Interactive views run on the Elm Architecture (TEA) in three layers with one-way dependencies. ADR-0005 records the decision.

| Layer | Location | Responsibility |
|-------|----------|---------------|
| Core | `src/update/<feature>.rs` | One `Model` per feature holding all session state, an exhaustive `Msg` enum, a data-only `Effect` enum, and a pure `update(msg, &mut model) -> Vec<Effect>`. Imports no serenity, tokio, diesel, or poise. |
| Shell | `src/bot/gui/` | A sealed `GuiFeature` trait (pure `view`, `translate`, `update` via the core) plus the `Host` event loop: collectors, acknowledgement, and the reply handle. |
| Adapter | Per feature, e.g. `src/bot/gui/voice_settings.rs` | One `EffectHandler` executing effects against services via `ctx.data().service`. Effect results return as `Msg`s (e.g. `SettingsPersisted`). |

The `sealed::Sealed` supertrait closes `GuiFeature` to external implementors — only `src/bot/gui/` may add features. One-shot commands (`register`, `unregister`) drive `view` + `update` directly without the Host. Snapshot tests in each feature pin the rendered component JSON.

#### Host Loop

1. **Construction**: The command handler fetches the required data into a `Config` and calls `Host::<Feature, _>::new(ctx, config, adapter, timeout, router)`. `Feature::initial(config)` builds the `Model`. Initial data loads are data-in at construction, not effects; only in-session async work (refetch, image regeneration, persistence) becomes an `Effect -> Msg` round trip.
2. **First frame**: The host applies the start message (`Feature::start_msg()`, the `Msg::Start` equivalent), renders through `Feature::view(&model, registry)`, and sends the message. `Feature::attachments` adds extra attachments to the reply, such as image bytes held in the model.
3. **Collectors**: The host claims the sent message with the translation layer (`ctx.data().translate_layer.host_session(msg_id)`, released when the loop ends) and starts a `ViewChannel` on it. It spawns only the collectors the feature enables through `channel_config()`: components, modals, messages, reactions.
4. **Event loop**: `Host::run()` selects on two channels — the collector's event channel and the host's message channel:
   - **Component interactions**: The `ViewChannel` resolves the `custom_id` back to an `Action` through the `ActionRegistry`. The host first consults `Feature::open_modal`: when the feature opens a modal, the host skips the acknowledge and the re-render — the modal itself already responds to the interaction — and the submission arrives later as a `Msg`. Otherwise `Feature::translate(action, values, &model)` maps the action and the select values to a `Msg`. Unknown ids are acknowledged and skipped.
   - **Other events**: Modals, messages, and reactions go through `Feature::on_event`, which returns a `Msg` or nothing. The collector timeout becomes `Feature::timeout_msg()` — expiry is one more update, not a special path.
   - **Effect follow-ups**: The adapter executes each returned effect. Fast effects return their result `Msg`s directly; slow effects `tokio::spawn` the work and deliver the result on the host's message channel.
5. **Update and render**: The host applies each `Msg` through `Feature::update` — the only writer of the model — executes the returned effects through the adapter, re-renders through `view`, and edits the live message.
6. **Exit**: `Feature::exit_navigation(msg)` returns the next `Navigation` when a message ends the feature (e.g. `Back` → `Navigation::Back`). The host navigates the router and the loop ends; the session loop then resolves the target — `Back` re-runs the parent frame, `SettingsMain` morphs the message into the settings plugin's hub and ends the session as a plugin view session, and a root `Back` dismisses the message (see `src/bot/command/session_exit.rs`).

#### Interaction Substrate (`src/bot/view/mod.rs`)

The substrate collects Discord events for the Host. It never renders and never mutates feature state.

| Component | Responsibility |
|-----------|---------------|
| `Action` | Trait for action enums: one variant per button or select option, each with a UI label. |
| `ActionRegistry` / `RegisteredAction` | Maps `Type:timestamp:counter` custom ids to actions; `RegisteredAction` builds the Discord components (`.as_button()`, `.as_select()`). |
| `SelectValues` | Select-menu values extracted from an interaction (string, channel, role, user). |
| `ViewEvent` | One event that wakes the host loop: component, modal, message, reaction, async, or timeout. |
| `ViewChannel` / `ViewChannelConfig` | Background collectors, spawned as tasks, that feed events into the loop. |

Custom-id helpers live in `src/bot/gui/input.rs` (`build_custom_id`, `parse_custom_id`).

#### Translation Layer (`src/bot/translate.rs`)

Every interactive view message belongs to exactly one live session runtime — a Host (TEA) session or a plugin view session — and exactly one runtime acknowledges each interaction on it. The translation layer owns that routing decision: it tracks the messages of live Host sessions (`TranslateLayer`, claimed by `Host::run` through an RAII `HostSession` guard), while the plugin engine keeps tracking its own sessions. The global event handler consults the layer before it responds:

| Message ownership | Acknowledgement owner | Global handler behavior |
|-------------------|----------------------|------------------------|
| Live Host session | The Host loop, after handling — except modal-triggering actions, where opening the modal is itself the response, and modal submissions, which the poise modal task the feature spawned acknowledges while the session is live (a submission that arrives after the session ends is stale: the global handler acknowledges it and routes it to the engine, poise's own ack then fails `AlreadyResponded`, and the feature's modal task swallows both) | Skips the interaction entirely |
| Everything else (plugin session, or no session) | The global handler, before the plugin round trip | Acknowledges, then routes through the plugin view engine (a "no open session" there means a genuinely stale view) |

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

## Plugin Subsystem

Plugins are subprocesses that speak JSON-lines over stdio through
`pwr-plugin-protocol`, with a `"t"` tag on every message. The host (this
monolith) spawns them via `PluginManager`, routes their Discord
interactions, and answers their `host.*` ops. The subsystem sits outside
the Layer Overview because plugins are peer processes of the layered
core, not a layer of it.

| Location | Role |
|----------|------|
| `crates/pwr-plugin-protocol` | Wire types: `Msg`, `Manifest`, `ViewSpec`, `HostCap`, `WireError` |
| `src/plugin/` | Plugin host: `manager` (spawn, health, respawn, unload), `interaction` (session engine), `host` (`host.*` ops), `command` (slash dispatch), `events` (gateway fan-out), `install` (pinned catalog), `view` (gate) |
| `crates/plugin/` | Plugins: `hello` (fixture), `settings` (settings core) |
| `crates/pwr-poise-components` | Reusable components library on pwr-ext (typed builders, pagination) |

### Plugin Data Flow

```
Slash command
  → plugin_slash_dispatch (src/plugin/command.rs)
  → subprocess invoke             correlated call over stdio
  → ViewSpec.data                 raw Discord message JSON
  → gate                          validate_view_data
  → Discord                       edit_original_interaction_response

Component / modal interaction
  → route_view_interaction (src/bot/mod.rs)
  → interact_validated            closure-supplied gate
  → plugin (view.interact)         returns a new ViewSpec
  → validated data                invalid data leaves prior view + last_active
  → commit_interaction_view        transactional commit
  → edit_message
```

### Validate-Only Gate

`validate_view_data` (`src/plugin/view.rs`) parses a clone of
`ViewSpec.data` through pwr-ext `CreateMessageDe` at all three raw-send
boundaries: initial dispatch, component and modal re-render, and
`host.open_view`. It discards the parsed value and sends the original JSON
verbatim. A failure is a `WireError` with kind `InvalidView`. The host
never partially sends, registers, or commits. An invalid re-render keeps
the prior session view and `last_active` unchanged.

### View Authoring Split

| View kind | Surface | Example |
|-----------|---------|---------|
| Fixed view | pwr-ext `view!` → `CreateMessage` → `ViewSpec.data` | `hello` view, settings `about_view`, settings hub |
| Runtime-assembled | pwr-ext `component!` + splices inside a `view!` literal | Settings hub nav row (0..N) |

No in-repo view is library-composed any more: `crates/pwr-poise-components`
no longer assembles a live view, and stays as the reusable library for
shared pieces the grammar does not fit (pagination, for example).

### Preview Loop

`./dev.sh preview` runs the `crates/preview` bin, an adapter over the
same protocol port. It spawns a plugin, captures its view, and renders the
payload to HTML/PNG under `.scratch/preview/` via `pwr-viewgen`.

Glossary terms live in `CONTEXT.md`. The gate and authoring decisions
live in ADR-0003 and ADR-0004.

---

## Event Lifecycles

### User Interaction

```
Discord interaction
  → Poise routes to command entry function
  → Router::new(ctx)
  → Router::run(initial)             starts navigation loop
      → CommandHandler::run(router)
          → Service::fetch(...)          fetch required data (boot-load)
          → build EffectHandler          adapter over ctx.data().service
          → Host::<Feature, _>::new(config, adapter, timeout).run()
              → Feature::initial(config) build the Model
              → update(start msg)         first transition
              → Feature::view(model)      build Discord components
              → [Host loop]
                  → ViewChannel event      component / modal / async / timeout
                  → Feature::open_modal?   modal trigger consumes interaction
                  → Feature::translate     action → Msg
                  → update(msg, model)     pure transition → Vec<Effect>
                  → EffectHandler::execute effects (spawned, results as Msgs)
                  → Feature::view(model)   re-render, edit reply
              → exit msg (timeout) or terminal Msg
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
| TEA core (`src/update`) | Application | Pure `update -> Vec<Effect>` transitions, single-source-of-truth Models |
| GuiFeature + Host (`src/bot/gui`) | Presentation | Sealed feature contract; host owns loop, acks, collectors |
| EffectHandler adapter | Application | The only place effects execute (services, image gen) |
| Strategy | Domain | Swappable platform implementations |
| Repository (factory) | Infrastructure | `Repos` trait with `PgRepos` concrete impl |
| Event Bus | Application | Decoupled pub/sub communication |
| Service | Application | Business logic via trait objects (`SettingsProvider`, `FeedSubscriptionProvider`, etc.) |
