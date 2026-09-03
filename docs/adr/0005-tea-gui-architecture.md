# Host interactive views run on the Elm Architecture

Host command views lived in a `ViewEngine` whose handlers mutated view
structs directly (`&mut self`), re-fetched data inside the view layer, and
kept two sources of truth for settings state (a domain model next to a raw
`&mut ServerSettings`). The engine also owned rendering, event collection,
and acknowledgement in one object. Six features each re-implemented the
bridge from an update `Cmd` to real side effects, and `voice/settings` had
no update module at all — its save-on-exit was implicit in handler
control flow.

We replaced the engine with a Rust TEA (Elm Architecture / MVU) runtime
split into three layers with one-way dependencies:

- **Core** (`src/update/<feature>.rs`): one `Model` per feature holding all
  session state, an exhaustive `Msg` enum, a data-only `Effect` enum, and a
  pure `update(msg, &mut model) -> Vec<Effect>`. The core imports no
  serenity, tokio, diesel, or poise — raw image bytes may live in the
  model, serenity types may not.
- **Shell** (`src/bot/gui/`): a sealed `GuiFeature` trait (pure `view`,
  `translate`, `update` via the core) plus a `Host` that owns the event
  loop, collectors, acknowledgement, and the reply handle. One-shot
  commands (register, unregister) drive `view` + `update` directly without
  the Host.
- **Adapters**: one `EffectHandler` per feature executing effects against
  services via `ctx.data().service`; effects return results as `Msg`s
  (`ImageRendered`, `SettingsPersisted`).

Initial data loads are shell-legal `Config` data-in at Host construction
(Elm's init arguments) — a `Start -> Query -> Loaded` first frame would
have forced a loading-state render and broken byte-identical output. Only
in-session async work (refetch, image regeneration, persistence) becomes
an `Effect -> Msg` round trip.

Two seams carry behavior the pure core cannot express. Modal interactions
need the component interaction itself, so `GuiFeature::open_modal` is a
defaulted host-level hook returning the `ViewCmd::AlreadyResponded`
equivalent (skip ack and re-render; deliver the submission as a `Msg`).
Persist timing follows each feature's old handler: welcome persists on
every mutation, voice and feed settings persist on terminal exits. Pattern
uniformity lost to behavior preservation; the choice is documented in the
respective core modules.

The migration was executed per feature behind characterization snapshots
that pin the rendered component JSON (custom_id timestamps normalized), so
every phase shipped with byte-identical rendering. All views already
authored through the `view_support` `component!` surface (ADR-0004 host
side) became the bodies of the pure `view(&Model)` functions unchanged.

The old `ViewEngine`, `ViewRender`, `ViewHandler`, `ViewCmd`, and
`ViewContext` are retired. The interaction substrate survives them:
`Action`, `ActionRegistry`, `SelectValues`, `ViewEvent`, `ViewChannel`,
and `SyntheticEvent` remain the collector machinery the `Host` runs on.
The plugin runtime (`src/plugin/**`, message-id keyed) is a separate
engine and is not affected by this decision.
