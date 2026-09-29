# Architecture

## Layer Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Presentation Layer                       │
│          bot/ — commands (Router → CommandHandler)          │
├─────────────────────────────────────────────────────────────┤
│                   Application Layer                         │
│              event/ — gateway and plugin events              │
├─────────────────────────────────────────────────────────────┤
│                    Service Layer                            │
│                service/ — business logic                    │
├─────────────────────────────────────────────────────────────┤
│                    Domain Layer                             │
│           entity/ — host data contracts                    │
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
| `crates/plugin/voice` manifest | `/vc` group and `/voice-settings` panel |
| `crates/plugin/feed` manifest | `/feed` group and `/feed-settings` |
| `settings.rs` | `/settings` — the host Settings GUI |
| `welcome/mod.rs` | `/welcome` |
| `about.rs` | `/about` |
| `register.rs` | `/register` |
| `register_owner.rs` | `/register_owner` |
| `unregister.rs` | `/unregister` |
| `dump_db.rs` | `/dump_db` |

### Router → CommandHandler → Host Flow

Interactive commands follow a **Router → CommandHandler → Host** flow:

- **`Router`** — receives the Poise context, owns navigation state, drives handlers. Its session loop keeps the frames open on the message: each target that runs becomes the newest frame, and `Back` closes the newest frame and re-runs the one beneath it, morphing the same message. Defined in `src/bot/command/mod.rs`.
- **`CommandHandler`** — trait for handler run loops for host-owned commands. Plugin commands use the manifest bridge instead.
- **`Navigation`** — enum signalling the next navigation step (e.g. `Back`, `Exit`, `SettingsSection`). `SettingsSection` hands the live message to a settings section's panel plugin (the session then parks until the panel's Back or About press returns or re-renders the message through the host-reserved `settings` or `about` `host.open_view` target), and a `Back` over the last open frame dismisses the message. The handoff and dismissal live in `src/bot/command/session_exit.rs`. Defined in `src/bot/navigation.rs`.
- **`Host`** — the TEA event loop that runs one interactive view. Defined in `src/bot/gui/rt.rs`.

### Interactive Views (TEA — `src/update/` cores + `src/bot/gui/` shell)

Interactive views run on the Elm Architecture (TEA) in three layers with one-way dependencies. ADR-0005 records the decision.

| Layer | Location | Responsibility |
|-------|----------|---------------|
| Core | `src/update/<feature>.rs` | One `Model` per host feature holding session state, an exhaustive `Msg` enum, a data-only `Effect` enum, and a pure `update(msg, &mut model) -> Vec<Effect>`. Imports no serenity, tokio, diesel, or poise. |
| Shell | `src/bot/gui/` | A sealed `GuiFeature` trait (pure `view`, `translate`, `update` via the core) plus the `Host` event loop: collectors, acknowledgement, and the reply handle. |
| Adapter | Per host feature | One `EffectHandler` executing effects against host services via `ctx.data().service`. |

The `sealed::Sealed` supertrait closes `GuiFeature` to external implementors — only `src/bot/gui/` may add features. One-shot commands (`register`, `unregister`) drive `view` + `update` directly without the Host. Snapshot tests in each feature pin the rendered component JSON.

Plugin views do not use the host's sealed `GuiFeature` or `Host` loop. The
feed and voice plugins keep pure update modules in their own `src/update/`
directories, render views in their own `src/view/` directories, and invoke
their repositories and services directly. The host runtime remains only for
host-owned views. After validating a complete `ViewSpec`, the host resolves
declared previews and decodes runtime files for the initial render and every
interaction edit.

#### Host Loop

1. **Construction**: The command handler fetches the required data into a `Config` and calls `Host::<Feature, _>::new(ctx, config, adapter, timeout, router)`. `Feature::initial(config)` builds the `Model`. Initial data loads are data-in at construction, not effects; only in-session async work (refetch, image regeneration, persistence) becomes an `Effect -> Msg` round trip.
2. **First frame**: The host applies the start message (`Feature::start_msg()`, the `Msg::Start` equivalent), renders through `Feature::view(&model, registry)`, and sends the message. `Feature::attachments` adds extra attachments to the reply, such as image bytes held in the model.
3. **Collectors**: The host claims the sent message with the translation layer (`ctx.data().translate_layer.host_session(msg_id)`, released when the loop ends) and starts a `ViewChannel` on it. It spawns only the collectors the feature enables through `channel_config()`: components, modals, messages, reactions.
4. **Event loop**: `Host::run()` selects on two channels — the collector's event channel and the host's message channel:
   - **Component interactions**: The `ViewChannel` resolves the `custom_id` back to an `Action` through the `ActionRegistry`. The host first consults `Feature::open_modal`: when the feature opens a modal, the host skips the acknowledge and the re-render — the modal itself already responds to the interaction — and the submission arrives later as a `Msg`. Otherwise `Feature::translate(action, values, &model)` maps the action and the select values to a `Msg`. Unknown ids are acknowledged and skipped.
   - **Other events**: Modals, messages, and reactions go through `Feature::on_event`, which returns a `Msg` or nothing. The collector timeout becomes `Feature::timeout_msg()` — expiry is one more update, not a special path.
   - **Effect follow-ups**: The adapter executes each returned effect. Fast effects return their result `Msg`s directly; slow effects `tokio::spawn` the work and deliver the result on the host's message channel.
5. **Update and render**: The host applies each `Msg` through `Feature::update` — the only writer of the model — executes the returned effects through the adapter, re-renders through `view`, and edits the live message.
6. **Exit**: `Feature::exit_navigation(msg)` returns the next `Navigation` when a message ends the feature (e.g. a section click → `Navigation::SettingsSection`, the section handoff). The host navigates the router and the loop ends; the session loop then resolves the target — `Back` re-runs the parent frame, `SettingsSection` invokes the section's plugin command, morphs the message into the returned panel view, and parks the session until the panel's Back or About press wakes it — the host-reserved `settings`/`about` open_view targets re-run the Settings GUI or the About view on the same message — and a root `Back` dismisses the message (see `src/bot/command/session_exit.rs`). The Settings view returns a plain `Navigation::Back`; on an empty stack it is the root dismissal.

#### Interaction Substrate (`src/bot/view/mod.rs`)

The substrate collects Discord events for the Host. It never renders and never mutates feature state.

| Component | Responsibility |
|-----------|---------------|
| `Action` | Trait for action enums: one variant per button or select option, each with a UI label. |
| `ActionRegistry` / `RegisteredAction` | Maps `Type:timestamp:counter` custom ids to actions; `RegisteredAction` builds the Discord components (`.as_button()`, `.as_select()`). |
| `SelectValues` | Select-menu values extracted from an interaction (string, channel, role, user). |
| `ViewEvent` | One event that wakes the host loop: component, modal, message, reaction, async, or timeout. |
| `ViewChannel` / `ViewChannelConfig` | Background collectors, spawned as tasks, that feed events into the loop. |

`ActionRegistry` generates the `Type:timestamp:counter` custom ids used by
host-owned components; the registry and collectors live in
`src/bot/view/mod.rs`.

#### Translation Layer (`src/bot/translate.rs`)

Every interactive view message belongs to exactly one live session runtime — a Host (TEA) session or a plugin view session — and exactly one runtime acknowledges each interaction on it. The translation layer owns that routing decision: it tracks the messages of live Host sessions (`TranslateLayer`, claimed by `Host::run` through an RAII `HostSession` guard), while the plugin engine keeps tracking its own sessions. The global event handler consults the layer before it responds:

| Message ownership | Acknowledgement owner | Global handler behavior |
|-------------------|----------------------|------------------------|
| Live Host session | The Host loop, after handling — except modal-triggering actions, where opening the modal is itself the response, and modal submissions, which the poise modal task the feature spawned acknowledges while the session is live (a submission that arrives after the session ends is stale: the global handler acknowledges it and routes it to the engine, poise's own ack then fails `AlreadyResponded`, and the feature's modal task swallows both) | Skips the interaction entirely |
| Everything else (plugin session, or no session) | The global handler, before the plugin round trip | Acknowledges, then routes through the plugin view engine (a "no open session" there means a genuinely stale view) |

ADR-0006 records this ownership decision, including the accepted ghost window between the Host claim drop and the Settings section handoff's plugin registration.

---

## Application Layer (`src/event/`)

Cross-cutting gateway and plugin-event concerns that don't belong to any single
feature. Glues layers together without containing business logic.

### Plugin event fan-out (`src/plugin/events.rs`)

The host converts gateway events into opaque plugin events. Manifest
`event_handlers` subscriptions route `voice_state`, `guild_create`, and
`view.timeout` to the voice plugin; the plugin owns the event subscriber and
session lifecycle.

| Event | Published by | Consumed by |
|-------|-------------|-------------|
| `voice_state` | `BotEventHandler` | voice plugin |
| `guild_create` | `BotEventHandler` | voice plugin |
| `view.timeout` | `InteractionEngine` | voice plugin |

---

## Service Layer (`src/service/`)

The only layer that enforces business rules. Handlers call services; services orchestrate repositories and platforms. Nothing above this layer touches data directly.

| Service (trait) | Responsibility |
|---------|---------------|
| `SettingsProvider` | Server configuration management |
| `InternalOps` | Bot metadata and internal operations |

---

## Domain Contracts (`src/entity.rs`, plugin crates)

The host keeps shared settings, bot metadata, plugin enablement, and the
transitional feed-dump DTOs. Feed and voice entities, repositories, services,
and migrations live in their plugin crates.

### Host entities (`src/entity.rs`)

| Entity | Description |
|--------|-------------|
| `FeedEntity` | Feed content source used by the transitional database dump |
| `FeedItemEntity` | Feed update used by the transitional database dump |
| `SubscriberEntity` | Feed notification target used by the transitional database dump |
| `FeedSubscriptionEntity` | Feed subscription link used by the transitional database dump |
| `ServerSettingsEntity` | Shared per-guild configuration and legacy settings |
| `BotMetaEntity` | Key-value bot metadata |
| `GuildPluginEntity` | Per-guild plugin enablement |

### Plugin entities

| Plugin | Entity home |
|--------|-------------|
| `feed` | `crates/plugin/feed/src/entity.rs` |
| `voice` | `crates/plugin/voice/src/entity.rs` |

### Feed Platforms (`crates/plugin/feed/src/feed/`)

The feed plugin owns the platform strategy and its service. The host does
not construct feed platforms or retain a platform registry.

| Platform | API |
|----------|-----|
| `MangaDexPlatform` | MangaDex |
| `AniListPlatform` | AniList |
| `ComickPlatform` | Comick |

---

## Infrastructure Layer (`src/repo/`)

Data access. Core repositories depend on host-owned entities and migrations;
plugin repositories depend on plugin-owned entities and migrations. The core
connection pool is shared through `HostConfig`, while each plugin owns its
Diesel query logic for its own tables.

A factory trait `Repos` defines the repo access interface. The concrete `PgRepos` struct holds per-table `Pg*Repo` handles and implements the factory:

```rust
pub trait Repos: Send + Sync {
    fn feed_dump(&self) -> Box<dyn FeedDumpRepository + Send + Sync>;
    fn server_settings(&self) -> Box<dyn ServerSettingsRepository + Send + Sync>;
    fn bot_meta(&self) -> Box<dyn BotMetaRepository + Send + Sync>;
    fn plugin_kv(&self) -> Box<dyn PluginKvRepository + Send + Sync>;
    fn guild_plugins(&self) -> Box<dyn GuildPluginRepository + Send + Sync>;
}

pub struct PgRepos {
    feed_dump: PgFeedDumpRepo,
    pub server_settings: PgServerSettingsRepo,
    pub bot_meta: PgBotMetaRepo,
    pub plugin_kv: PgPluginKvRepo,
    pub guild_plugins: PgGuildPluginRepo,
    pool: DbPool,
}
```

`PgFeedDumpRepo` is read-only and exists for the transitional `/dump_db`
projection. The feed and voice plugins own their writes, migrations, and
repositories under their respective crates.

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
| `crates/pwr-plugin-protocol` | Wire types: `Msg`, `Manifest`, `ViewSpec`, `HostOp`, `WireError` |
| `src/plugin/` | Plugin host: `manager` (spawn, health, respawn, unload), `interaction` (session engine), `host` (`host.*` ops), `command` (slash dispatch), `events` (gateway fan-out), `install` (pinned catalog), `view` (gate) |
| `crates/plugin/` | Plugins: `hello` (fixture), `feed` (subscriptions and delivery), `voice` (tracking, statistics, leaderboard, settings), `welcome` (settings panel) |
| `crates/pwr-poise-components` | Reusable components library on pwr-ext (typed builders, pagination) |

### Plugin Data Flow

```
Slash command
  → plugin_slash_dispatch (src/plugin/command.rs)
  → subprocess invoke             correlated call over stdio
  → ViewSpec.data                 raw Discord message JSON
  → gate                          validate_view_spec (data + files)
  → PreviewResolver               fill declared attachment slots
  → ViewSpec.files                decode after the gate and attach runtime files
  → Discord                       edit_original_interaction_response

Component / modal interaction
  → route_view_interaction (src/bot/mod.rs)
  → interact_validated            closure-supplied gate
  → plugin (view.interact)         returns a new ViewSpec
  → validated whole ViewSpec        invalid data or files leave prior view + last_active
  → commit_interaction_view        transactional commit
  → edit_message with attachments
```

### Validate-Only Gate

`validate_view_spec` (`src/plugin/host.rs`) parses a clone of
`ViewSpec.data` through pwr-ext `CreateMessageDe` and validates the complete
runtime-file list at every raw-send boundary: initial dispatch, component and
modal re-render, and the Settings section handoff's message morph. It discards
the parsed message value and sends the original JSON verbatim. A failure is a
`WireError` with kind `InvalidView`. The host never partially sends, registers,
or commits. The combined preview and runtime attachment count is limited to 10.
An invalid re-render keeps the prior session view and `last_active` unchanged.

### View Authoring Split

| View kind | Surface | Example |
|-----------|---------|---------|
| Fixed view | pwr-ext `view!` → `CreateMessage` → `ViewSpec.data` | `hello` view |
| Runtime-assembled | pwr-ext `component!` + splices inside a `view!` literal | panel plugin views (conditional rows, 0..N) |

No in-repo view is library-composed any more: `crates/pwr-poise-components`
no longer assembles a live view, and stays as the reusable library for
shared pieces the grammar does not fit (pagination, for example).

### Preview Loop

`./dev.sh preview` runs the `crates/preview` bin, an adapter over the
same protocol port. It spawns a plugin, captures its view, and renders the
payload to HTML/PNG under `.scratch/preview/` via `pwr-viewgen`.

Glossary terms live in `CONTEXT.md`. The gate and authoring decisions
live in ADR-0003 and ADR-0004; ack ownership, the first-response shape,
and the offline test strategy live in ADR-0006, ADR-0007, and
ADR-0008.

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
Feed plugin SeriesFeedPublisher (scheduled)
  → Platform::fetch_latest()         poll external API
  → plugin FeedSubscriptionService   validate, find subscribers
  → plugin EventBus                  publish FeedUpdateEvent
  → plugin DM/Guild subscribers      host.send_message
```

### Voice State Change

```
Discord gateway event
  → BotEventHandler::dispatch()
  → PluginEventRouter::fan_out("voice_state")
  → voice plugin VoiceStateSubscriber
  → VoiceTrackingService             update session state
  → voice plugin repository          persist to PostgreSQL
```

---

## Design Patterns Summary

| Pattern | Where | Purpose |
|---------|-------|---------|
| Router → CommandHandler | Presentation | Navigation loop driving per-domain handlers |
| TEA core (`src/update`) | Application | Pure `update -> Vec<Effect>` transitions, single-source-of-truth Models |
| GuiFeature + Host (`src/bot/gui`) | Presentation | Sealed feature contract; host owns loop, acks, collectors |
| EffectHandler adapter | Application | Host features execute effects through service/image adapters; plugin commands invoke their own services directly |
| Strategy | Domain | Swappable platform implementations |
| Repository (factory) | Infrastructure | `Repos` trait with `PgRepos` concrete impl |
| PluginEventRouter | Application | Decoupled gateway-event fan-out to subscribed plugins |
| Service | Application | Business logic via trait objects (`SettingsProvider`, `InternalOps`) |
