# Discord Views Architecture Redesign (v2.0)

This document outlines the proposed architectural redesign for Discord Components interactive views. The primary goals are to allow fully asynchronous, non-blocking interaction handling, abstract away specific collector implementations, decouple child view registries from traits, and significantly reduce boilerplate for implementors.

## Core Components

### `ViewEvent<T>`
An enum that represents any event that can wake up the view loop. By unifying events here, the view loop (`ViewEngine`) can multiplex over a single receiver.
- **`Component(T, ComponentInteraction)`**: Emitted when a user interacts with a button or select menu.
- **`Modal(T, ModalInteraction)`**: Emitted when a user submits a modal.
- **`Message(T, Message)`**: Emitted when a message matches a collector criteria (optional).
- **`Async(T)`**: Emitted by background tasks to dispatch new actions into the view loop.

### `Trigger<'a>`
A borrowed representation of the event source, passed into `handle()`. This allows the view handler to access the raw interaction to `create_response()` or defer it.
- **`Component(&'a ComponentInteraction)`**
- **`Modal(&'a ModalInteraction)`**
- **`Message(&'a Message)`**
- **`Async`**

### `ViewContext<'a, T>`
Provides the operational context to the handler. 
- Contains the `poise::Context`.
- Contains an `mpsc::UnboundedSender<ViewEvent<T>>`, allowing the handler to spawn `tokio::task` futures that send events back.
- **Responsibility**: Expose API to safely mutate view state from the background without blocking the loop.

### `ActionRegistry<T>`
Registers `T: Action` mapping to a string ID.
- *New Responsibility*: Resolving child views directly. Instead of `ViewHandler` providing a `children()` vec, `ActionRegistry` can register other registries recursively.

### `ViewRender`
Trait defining how the state translates to Discord UI.
- **Responsibility**: Provides a `render(&self) -> ResponseKind` method.
- *Not its responsibility*: Tracking message IDs, editing messages, logic.

### `ViewHandler<T>`
Trait defining the business logic and state mutations.
- **Responsibility**: Provides `handle(&mut self, action: T, trigger: Trigger<'_>, ctx: &ViewContext<'_, T>) -> Result<ViewCommand, Error>`.
- *Not its responsibility*: Resolving children, rendering, editing messages.

### `ViewEngine<'a, T, V, H>`
The centralized event runner that replaces `InteractiveViewBase`.
- **Responsibilities**:
  - Initializes `mpsc` channel.
  - Spawns interaction collectors (by default, `ComponentInteractionCollector`).
  - Calls `initial_render()`.
  - Runs `tokio::select!` over the receiver.
  - Handles the `ViewCommand::Render` returned from handlers by editing the original message.

## Things that are NOT their responsibilities (Out of scope)
- **Automatic Deferral**: The framework will not automatically `defer()` all events. It's up to the handler using the `Trigger` to defer or acknowledge if necessary, though `ViewEngine` may auto-acknowledge component interactions if configured.
- **Deep nesting logic in Engine**: The `ViewEngine` only cares about the root `Action`. Child delegation happens at the registry parsing level.

## Migration Phases

### Phase 1: Foundation
1. Refactor `@src/bot/views.rs` to include `ViewEvent`, `Trigger`, `ViewContext`, `ViewEngine`, `ViewRender`, and `ViewHandler`.
2. Update `ActionRegistry` to support `add_child`.
3. Optionally modify it when the designs need to be adjusted (e.g. lifetimes or async traits).

### Phase 2: Macro Adjustments
1. Update `impl_interactive_view!` and `view_core!` in `@src/macros.rs` to align with the new structure. Ensure it generates the `ViewRender` trait automatically, or just let users write it manually depending on the design ergonomics.

### Phase 3: Simple Views (`about.rs`)
1. Implement the simple, non-async view to prove the basic event loop works.

### Phase 4: Child Views (`pagination.rs` and `feed/list.rs`)
1. Refactor `pagination.rs` to support being a child registry.
2. Update `list.rs` to compose the `pagination` registry and logic.

### Phase 5: Async & Modals (`welcome.rs`)
1. Refactor `welcome.rs` to utilize the `ViewContext::spawn` or channel sender.
2. Prove that the main loop doesn't block while waiting for the modal submission.

## Migration Instructions
- Review `@src/bot/views.rs` iteratively. If trait bounds become too complex or lifetimes clash in `ViewEngine`, fall back to a more concrete struct approach.
- Ensure `Cargo.toml` and imports (`use tokio::sync::mpsc`) are correctly managed.
- Once migration proves stable on `welcome.rs`, we can propagate to other views.