# Discord Views Architecture Redesign (v2.0)

This document outlines the finalized architectural redesign for Discord Components interactive views. The primary goals are to allow fully asynchronous, non-blocking interaction handling, abstract away specific collector implementations, decouple child view registries from traits, and significantly reduce boilerplate for implementors.

## Core Components

### `ViewEvent<T>`
An enum that represents any event that can wake up the view loop. By unifying events here, the view loop (`ViewEngine`) can multiplex over a single receiver.
- **`Component(T, ComponentInteraction)`**: Emitted when a user interacts with a button or select menu.
- **`Modal(T, ModalInteraction)`**: Emitted when a user submits a modal.
- **`Message(T, Message)`**: Emitted when a message matches a collector criteria (optional).
- **`Async(T)`**: Emitted by background tasks to dispatch new actions into the view loop.
- **`Timeout`**: Emitted when the interaction collector times out.

### `Trigger<'a>`
A borrowed representation of the event source, passed into `handle()`. This allows the view handler to access the raw interaction to `create_response()` or defer it without taking ownership.
- **`Component(&'a ComponentInteraction)`**
- **`Modal(&'a ModalInteraction)`**
- **`Message(&'a Message)`**
- **`Async`**
- **`Timeout`**

### `ViewSender<T>` & Context Mapping
A trait abstracting the ability to send `ViewEvent<T>`. This powers the **Delegation Pattern** without requiring `children()` trait leaks.
- `ViewContext` holds an `Arc<dyn ViewSender<T>>`.
- Calling `ctx.map(ParentAction::Child)` creates a new context for the child. 
- The child handles its own events, and `MappedSender` automatically wraps them in the parent's `Action` enum before passing them back to the main loop.

### `ViewCommand`
The enum returned by handlers to dictate engine flow:
- **`Render`**: Tells the engine to re-render the view components and update the message.
- **`Exit`**: Breaks the `ViewEngine` loop immediately (skipping `on_action` callbacks).
- **`Continue`**: Does not re-render or exit, but lets the action fall through to the `on_action` callback (useful for navigation and parent coordinators).
- **`AlreadyResponded`**: Like `Continue`, but also explicitly tells the `ViewEngine` *not* to auto-acknowledge the interaction (vital for actions that open Modals).

### `ActionRegistry<T>`
Registers `T: Action` mapping to a string ID.
- Resolves Discord `custom_id`s back into enum actions.
- Passed directly to `render()` and `create_reply()` to dynamically generate components.

### `ViewRender<T>`
Trait defining how the state translates to Discord UI.
- **Responsibility**: Provides a `render(&self, registry: &mut ActionRegistry<T>) -> ResponseKind` method.
- *Not its responsibility*: Tracking message IDs, editing messages, logic.

### `ViewHandler<T>`
Trait defining the business logic and state mutations.
- **Responsibility**: Provides `handle(&mut self, action: T, trigger: Trigger<'_>, ctx: &ViewContext<'_, T>) -> Result<ViewCommand, Error>`.
- *Not its responsibility*: Resolving children registries, rendering, editing messages.

### `ViewEngine<'a, T, H>`
The centralized event runner that replaces `InteractiveViewBase`.
- **Responsibilities**:
  - Initializes `mpsc` channel for async/modal events.
  - Spawns interaction collectors (by default, `ComponentInteractionCollector`).
  - Calls `render_view()` initially and when commanded.
  - Runs `tokio::select!` over the component stream and the `mpsc` channel.
  - Conditionally auto-acknowledges interactions (unless `AlreadyResponded` is requested).

## Successes & Completed Migration
- **Delegation Pattern**: Child views (like `PaginationViewV2`) can be mounted purely through `child.handle(...)` and `ctx.map()`. The parent Controller remains completely unaware of the child view.
- **Modal Support**: `welcome.rs` demonstrates using `ctx.spawn` to capture modal input via `.await` without blocking the main event loop, while returning `ViewCommand::AlreadyResponded` to prevent breaking Discord's modal flow.
- **Navigation Safety**: Coordinators can receive `ViewCommand::Continue` safely for actions like `Back` and `About`, permitting them to handle screen transitions outside of the internal view logic.