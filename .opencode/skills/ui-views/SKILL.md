---
name: ui-views
description: Create interactive UI views using Discord Components V2. Covers the three-trait system (View, ResponseView, InteractiveView), action_enum!, view_core! macros, ViewHandler trait, and the Controller pattern for building interactive Discord interfaces.
---

# UI Views (Components V2)

## Overview

This skill covers creating interactive UI views using Discord's Components V2 system. Views use three interconnected traits to handle UI rendering and user interactions, with a Controller pattern for complex flows.

## Core Traits

| Trait | Purpose |
|-------|---------|
| `View<'a, T>` | Core trait providing access to `ViewCore` (via `view_core!` macro) |
| `ResponseView<'a>` | Creates Discord components/embeds via `create_response()` |
| `InteractiveView<'a, T>` | Handles user interactions via a handler |
| `ViewHandler<T>` | Handles actions and timeouts for interactive views |

## View Creation Steps

### Step 1: Define Actions with `action_enum!`

Define the user actions your view can handle:

```rust
use crate::action_enum;

action_enum! {
    MyAction {
        #[label = "Click Me"]
        ButtonClick,
        #[label = "❮ Back"]
        Back,
    }
}
```

- Each variant becomes an action type
- `#[label]` sets the button/select label text

### Step 2: Create View Struct with `view_core!`

```rust
use std::time::Duration;
use crate::view_core;

view_core! {
    timeout = Duration::from_secs(120),
    /// Description of your view
    pub struct MyView<'a> {
        pub counter: i32,
    }
}
```

- `timeout`: Interaction timeout duration
- The struct holds your view's state data

### Step 3: Implement Constructor

```rust
impl<'a> MyView<'a> {
    pub fn new(ctx: &'a Context<'a>, counter: i32) -> Self {
        Self {
            counter,
            core: Self::create_core(ctx),
        }
    }
}
```

- Create `ViewCore` using `Self::create_core(ctx)`
- Pass any initial state data

### Step 4: Implement ResponseView

```rust
use crate::bot::views::{ResponseView, ResponseKind};

impl<'a> ResponseView<'a> for MyView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        let components = vec![
            CreateComponent::TextDisplay(
                CreateTextDisplay::new(format!("Counter: {}", self.counter))
            ),
            CreateComponent::ActionRow(CreateActionRow::Buttons(vec![
                self.register(MyAction::ButtonClick)
                    .as_button()
                    .style(ButtonStyle::Primary),
                self.register(MyAction::Back)
                    .as_button()
                    .style(ButtonStyle::Secondary),
            ].into())),
        ];
        components.into()
    }
}
```

- Build Discord components using builder patterns
- Use `self.register(action).as_button()` to create buttons
- Return `ResponseKind` (components, embeds, or both)

### Step 5: Implement ViewHandler

Instead of implementing `InteractiveView::handle` directly, implement `ViewHandler`:

```rust
use crate::bot::views::ViewHandler;
use serenity::all::ComponentInteraction;

#[async_trait::async_trait]
impl ViewHandler<MyAction> for MyViewHandler {
    async fn handle(
        &mut self,
        action: &MyAction,
        _interaction: &ComponentInteraction,
    ) -> Option<MyAction> {
        match action {
            MyAction::ButtonClick => {
                self.counter += 1;
                Some(action.clone())
            }
            MyAction::Back => Some(action.clone()),
        }
    }

    async fn on_timeout(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
```

### Step 6: Implement InteractiveView

```rust
use crate::bot::views::InteractiveView;
use futures::future::BoxFuture;

#[async_trait::async_trait]
impl<'a> InteractiveView<'a, MyAction> for MyView<'a> {
    type Handler = MyViewHandler;

    fn handler(&mut self) -> &mut Self::Handler {
        &mut self.handler
    }

    async fn run<F>(&mut self, mut on_action: F) -> Result<(), Error>
    where
        F: FnMut(MyAction) -> BoxFuture<'a, ViewCommand> + Send + Sync,
    {
        loop {
            let Some((action, interaction)) = self.listen_once(self.handler()).await? else {
                break;
            };

            let cmd = on_action(action).await;
            match cmd {
                ViewCommand::Render => {
                    self.render().await?;
                }
                ViewCommand::Exit => break,
            }
        }
        Ok(())
    }
}
```

## Key View Components

### Containers
`CreateContainer` - Groups components together

### TextDisplay
`CreateTextDisplay` - Displays markdown text

```rust
CreateComponent::TextDisplay(
    CreateTextDisplay::new("## Header\nSome *markdown* content")
)
```

### Sections
`CreateSection` - Side-by-side layout

```rust
CreateComponent::Section(CreateSection::new()
    .add_component(component_a)
    .add_component(component_b))
```

### Buttons
`CreateButton` - Interactive buttons

```rust
self.register(MyAction::Click)
    .as_button()
    .style(ButtonStyle::Primary)  // Primary, Secondary, Success, Danger
    .emoji('🔥')
    .disabled(false)
```

Button styles:
- `ButtonStyle::Primary` - Blue/filled
- `ButtonStyle::Secondary` - Gray/outlined
- `ButtonStyle::Success` - Green
- `ButtonStyle::Danger` - Red

### Select Menus
`CreateSelectMenu` - Dropdown menus

```rust
self.register(MyAction::Select)
    .as_select(SelectMenuKind::String)
    .add_option("option1", "Option 1")
    .add_option("option2", "Option 2")
```

## View Utilities

### RenderExt::render()

Automatically send or edit the view:

```rust
use crate::bot::views::RenderExt;

// Send new message or edit existing
view.render().await?;
```

### InteractiveViewBase::listen_once()

Wait for a single interaction:

```rust
// Returns Option<(Action, ComponentInteraction)>
if let Some((action, interaction)) = self.listen_once(handler).await {
    // Handle the interaction
}
```

### register(action)

Registers an action and returns `RegisteredAction` with conversion methods:

```rust
// Convert to button
self.register(MyAction::Click).as_button()

// Convert to select menu
self.register(MyAction::Select).as_select(SelectMenuKind::String)

// Convert to select option
self.register(MyAction::Option).as_select_option()
```

### ViewCommand

Returned by the action callback to control the view loop:

```rust
use crate::bot::views::ViewCommand;

match action {
    MyAction::Click => ViewCommand::Render,  // Re-render and continue
    MyAction::Back => ViewCommand::Exit,     // Exit the view
}
```

## Complete Example

```rust
use std::time::Duration;
use std::sync::Arc;
use futures::future::BoxFuture;

use crate::view_core;
use crate::action_enum;
use crate::bot::views::{
    View, ResponseView, InteractiveView, ResponseKind, RenderExt,
    ViewHandler, ViewCommand, InteractiveViewBase,
};
use serenity::all::ComponentInteraction;

action_enum! {
    CounterAction {
        #[label = "Increment"]
        Increment,
        #[label = "Reset"]
        Reset,
    }
}

view_core! {
    timeout = Duration::from_secs(60),
    pub struct CounterView<'a> {
        pub count: i32,
    }
}

pub struct CounterViewHandler {
    count: i32,
}

#[async_trait::async_trait]
impl ViewHandler<CounterAction> for CounterViewHandler {
    async fn handle(
        &mut self,
        action: &CounterAction,
        _interaction: &ComponentInteraction,
    ) -> Option<CounterAction> {
        match action {
            CounterAction::Increment => {
                self.count += 1;
                Some(action.clone())
            }
            CounterAction::Reset => {
                self.count = 0;
                Some(action.clone())
            }
        }
    }
}

impl<'a> CounterView<'a> {
    pub fn new(ctx: &'a Context<'a>) -> Self {
        Self {
            count: 0,
            core: Self::create_core(ctx),
            handler: CounterViewHandler { count: 0 },
        }
    }
}

impl<'a> ResponseView<'a> for CounterView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        let components = vec![
            CreateComponent::TextDisplay(
                CreateTextDisplay::new(format!("**Count: {}**", self.count))
            ),
            CreateComponent::ActionRow(CreateActionRow::Buttons(vec![
                self.register(CounterAction::Increment)
                    .as_button()
                    .style(ButtonStyle::Primary),
                self.register(CounterAction::Reset)
                    .as_button()
                    .style(ButtonStyle::Danger),
            ].into())),
        ];
        components.into()
    }
}

#[async_trait::async_trait]
impl<'a> InteractiveView<'a, CounterAction> for CounterView<'a> {
    type Handler = CounterViewHandler;

    fn handler(&mut self) -> &mut Self::Handler {
        &mut self.handler
    }

    async fn run<F>(&mut self, mut on_action: F) -> Result<(), Error>
    where
        F: FnMut(CounterAction) -> BoxFuture<'a, ViewCommand> + Send + Sync,
    {
        loop {
            let Some((action, _)) = self.listen_once(self.handler()).await? else {
                break;
            };

            let cmd = on_action(action).await;
            match cmd {
                ViewCommand::Render => {
                    self.render().await?;
                }
                ViewCommand::Exit => break,
            }
        }
        Ok(())
    }
}
```

## Imports Reference

```rust
// Core macros
use crate::view_core;
use crate::action_enum;

// Views
use crate::bot::views::{View, ResponseView, InteractiveView, ResponseKind};
use crate::bot::views::{RenderExt, ViewHandler, ViewCommand};
use crate::bot::views::{InteractiveViewBase, RegisteredAction};

// Components
use serenity::all::{CreateComponent, CreateTextDisplay, CreateActionRow, CreateButton, ButtonStyle};
```

## Controller Pattern (MVC-C)

The Controller Pattern provides a structured architecture for building complex interactive flows. Coordinator manages `Context` and state while Controllers implement logic and return `NavigationResult`.

**Architecture**: `Coordinator` -> `Controller` -> `View` -> `NavigationResult`

### Creating a Controller

```rust
use crate::controller;

controller! { pub struct MyController<'a> {} }
```

### Implementing Controller Trait

```rust
use crate::bot::controller::{Controller, Coordinator};
use crate::bot::navigation::NavigationResult;

#[async_trait]
impl<S: Send + Sync + 'static> Controller<S> for MyController<'_> {
    async fn run(&mut self, coord: &mut Coordinator<'_, S>) -> Result<NavigationResult, Error> {
        let ctx = *coord.context();
        
        // Create and render a view
        let mut view = MyView::new(&ctx);
        view.render().await?;
        
        // Handle interactions in a loop
        view.run(|action| Box::pin(async move {
            // Handle action and return ViewCommand
            match action {
                MyAction::Continue => ViewCommand::Render,
                MyAction::Back => return NavigationResult::Back.into(),
                MyAction::Exit => return NavigationResult::Exit.into(),
            }
        })).await?;
        
        Ok(NavigationResult::Exit)
    }
}
```

### NavigationResult

Use `NavigationResult` to control flow:

```rust
use crate::bot::navigation::NavigationResult;

// Settings navigation
NavigationResult::SettingsMain
NavigationResult::SettingsFeeds
NavigationResult::SettingsVoice
NavigationResult::SettingsAbout

// Feed navigation
NavigationResult::FeedSubscriptions { send_into }
NavigationResult::FeedSubscribe { links, send_into }
NavigationResult::FeedUnsubscribe { links, send_into }
NavigationResult::FeedList(send_into)

// Voice navigation
NavigationResult::VoiceLeaderboard

// Universal navigation
NavigationResult::Back   // Go back to previous view
NavigationResult::Exit  // Exit this controller
```

### Coordinator Loop

```rust
pub async fn my_command(ctx: Context<'_>) -> Result<(), Error> {
    let mut coordinator = Coordinator::new(ctx);
    let mut controller = MyController::new(&ctx);
    let _result = controller.run(&mut coordinator).await?;
    Ok(())
}
```

### Guidelines

1. **Navigation**: Use `NavigationResult` unified enum for all navigation decisions
2. **Context**: Clone context `let ctx = *coord.context()` to avoid borrows
3. **View Loop**: Use `view.run()` with callback for handling actions
4. **Entry Point**: Create a wrapper function that initializes coordinator and controller

### Controller State

Controllers can hold state for the duration of a flow:

```rust
controller! {
    pub struct SettingsController<'a> {
        pub selected_category: Option<String>,
        pub page: u32,
    }
}

impl<'a> SettingsController<'a> {
    pub fn new() -> Self {
        Self {
            selected_category: None,
            page: 0,
        }
    }
}
```

## Best Practices

1. **Keep views focused** - One view per feature/command
2. **Handle timeouts** - Implement `on_timeout` in ViewHandler
3. **Validate input** - Check interaction author matches command author
4. **Use proper state** - Store all UI state in the view struct
5. **Return commands wisely** - Use `ViewCommand::Render` to re-render, `ViewCommand::Exit` to end
6. **Separate concerns** - Use ViewHandler for action logic, keep view for rendering

## Troubleshooting

### View not responding
- Check `timeout` is set appropriately
- Ensure `ViewHandler::handle` returns `Some(action)` to re-render

### Components not showing
- Verify `create_response` returns properly formatted `ResponseKind`
- Check component limits (5 per action row, 5 action rows per message)

### Interaction errors
- Ensure interaction is acknowledged within 3 seconds
- Set `should_acknowledge` on `InteractiveViewBase` to control auto-ack

### Navigation issues
- Use `NavigationResult::Exit` to end a controller
- Use `NavigationResult::Back` to return to previous controller (if using coordinator stack)
