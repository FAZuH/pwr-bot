---
name: ui-views
description: Create interactive UI views using the ViewEngine architecture. Covers ViewRender, ViewHandler, ActionRegistry, and the delegation pattern for modular Discord interfaces.
---

# UI Views (ViewEngine Architecture)

## Overview

The `ViewEngine` system provides a robust, asynchronous framework for building Discord UI components. It separates rendering from logic and supports complex features like background tasks, modals, and nested views.

## Core Components

| Component | Description |
|-----------|-------------|
| `Action` | An enum representing user interactions (buttons, select menus). |
| `ViewRender<T>` | Trait for generating Discord UI from state. |
| `ViewHandler<T>` | Trait for processing actions and updating state. |
| `ViewEngine<T, H>` | Orchestrates the event loop and interaction handling. |
| `ViewContext<T>` | Provides tools for async tasks and view delegation. |

## Implementation Steps

### 1. Define Actions
Create an enum for your view's actions and implement the `Action` trait.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MyAction {
    Click,
    Back,
}

impl Action for MyAction {
    fn label(&self) -> &'static str {
        match self {
            MyAction::Click => "Click Me",
            MyAction::Back => "Back",
        }
    }
}
```

### 2. Create the Handler Struct
This struct holds your view's state.

```rust
pub struct MyHandler {
    pub count: i32,
}
```

### 3. Implement `ViewRender`
Translate state into Discord components. Use the `ActionRegistry` to register buttons and menus.

```rust
impl ViewRender<MyAction> for MyHandler {
    fn render(&self, registry: &mut ActionRegistry<MyAction>) -> ResponseKind<'_> {
        let components = vec![
            CreateActionRow::Buttons(vec![
                registry.register(MyAction::Click).as_button().style(ButtonStyle::Primary),
                registry.register(MyAction::Back).as_button().style(ButtonStyle::Secondary),
            ]).into()
        ];
        
        CreateEmbed::new()
            .title("My View")
            .description(format!("Count: {}", self.count))
            .into()
    }
}
```

### 4. Implement `ViewHandler`
Process actions and return a `ViewCommand`.

```rust
#[async_trait::async_trait]
impl ViewHandler<MyAction> for MyHandler {
    async fn handle(
        &mut self,
        action: MyAction,
        trigger: Trigger<'_>,
        ctx: &ViewContext<'_, MyAction>,
    ) -> Result<ViewCommand, Error> {
        match action {
            MyAction::Click => {
                self.count += 1;
                Ok(ViewCommand::Render)
            }
            MyAction::Back => Ok(ViewCommand::Exit),
        }
    }
}
```

### 5. Run the View
Use `ViewEngine` in your controller or command.

```rust
pub async fn run_my_view(ctx: &Context<'_>) -> Result<(), Error> {
    let handler = MyHandler { count: 0 };
    let mut engine = ViewEngine::new(ctx, handler, Duration::from_secs(60));
    
    engine.run(|action| Box::pin(async move {
        // Optional callback for parent coordination
        ViewCommand::Continue
    })).await
}
```

## Advanced Patterns

### Async Tasks
Use `ctx.spawn` to run background tasks that dispatch actions back to the view loop.

```rust
ctx.spawn(async move {
    tokio::time::sleep(Duration::from_secs(5)).await;
    Some(MyAction::Click) // Dispatches a Click action automatically
});
```

### Modals
To open a modal, return `ViewCommand::AlreadyResponded` to prevent the engine from auto-acknowledging the interaction.

```rust
if let Trigger::Component(i) = trigger {
    i.create_response(ctx.poise.http(), CreateInteractionResponse::Modal(my_modal)).await?;
    return Ok(ViewCommand::AlreadyResponded);
}
```

### Child View Delegation
Map child actions to parent actions using `ctx.map`.

```rust
// In ParentHandler::handle
if let ParentAction::Pagination(child_action) = action {
    return self.pagination_view.handle(child_action, trigger, &ctx.map(ParentAction::Pagination)).await;
}
```
