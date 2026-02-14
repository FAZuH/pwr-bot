//! Controller pattern for managing interactive command flows.
//!
//! This module provides the Controller trait and Coordinator for managing
//! interactive controllers in Discord commands. The pattern uses dependency
//! inversion - controllers receive a coordinator to manage message lifecycle,
//! keeping controllers focused on their interactive logic.

use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::navigation::NavigationResult;

/// Provides context and state management to controllers.
///
/// The Coordinator provides access to Discord context and state for controllers.
/// Views handle their own message lifecycle via [`StatefulView`](crate::bot::views::StatefulView).
/// Controllers receive a reference to the coordinator to access context and state.
///
/// # Type Parameters
///
/// - `S`: The state type stored in this coordinator (default: `()`)
///
/// # Lifecycle
///
/// 1. Create coordinator with [`Coordinator::new`]
/// 2. Views handle sending/editing via [`StatefulView`](crate::bot::views::StatefulView)
/// 3. Access Discord context via [`Coordinator::context`]
/// 4. Access state via [`Coordinator::state`] and [`Coordinator::state_mut`]
///
/// # Example
///
/// ```ignore
/// let mut coordinator = Coordinator::new(ctx, initial_state);
/// let mut controller = SettingsController::new(coordinator.context());
///
/// let action = controller.run(&mut coordinator).await?;
/// ```
pub struct Coordinator<'a, S = ()> {
    ctx: Context<'a>,
    state: S,
}

impl<'a> Coordinator<'a, ()> {
    /// Creates a new coordinator for the given context without state.
    ///
    /// # Parameters
    ///
    /// - `ctx`: The Discord command context
    ///
    /// # Example
    ///
    /// ```ignore
    /// let coordinator = Coordinator::new(ctx, ());
    /// ```
    pub fn new(ctx: Context<'a>) -> Self {
        Self { ctx, state: () }
    }
}

impl<'a, S> Coordinator<'a, S> {
    /// Creates a new coordinator for the given context with state.
    ///
    /// # Parameters
    ///
    /// - `ctx`: The Discord command context
    /// - `state`: Initial state for this coordinator
    ///
    /// # Example
    ///
    /// ```ignore
    /// let coordinator = Coordinator::with_state(ctx, my_state);
    /// ```
    pub fn with_state(ctx: Context<'a>, state: S) -> Self {
        Self { ctx, state }
    }

    /// Returns the Discord command context.
    ///
    /// Provides access to Discord APIs, user information, and other
    /// context needed by controllers.
    ///
    /// # Returns
    ///
    /// A reference to the command context.
    pub fn context(&self) -> &Context<'a> {
        &self.ctx
    }

    /// Returns a reference to the coordinator state.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Returns a mutable reference to the coordinator state.
    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    /// Consumes the coordinator and returns the state.
    pub fn into_state(self) -> S {
        self.state
    }
}

/// Core trait for controllers.
///
/// Controllers manage interactive flows and return navigation results.
/// They receive a coordinator reference to update views, access context,
/// and read/write coordinator state.
///
/// Controllers are unaware of navigation flow - they simply execute their
/// logic and return navigation results. The coordinator decides what to do
/// with those results.
///
/// # Lifecycle
///
/// 1. Controller is created with necessary dependencies
/// 2. `run()` is called with a coordinator reference
/// 3. Controller sends initial view via coordinator
/// 4. Controller handles user interactions
/// 5. Controller returns `NavigationResult` when complete
///
/// # Example
///
/// ```ignore
/// struct SettingsController<'a> {
///     ctx: &'a Context<'a>,
///     view: SettingsView<'a>,
/// }
///
/// #[async_trait::async_trait]
/// impl<'a, S> Controller<S> for SettingsController<'a> {
///     async fn run(&mut self, coordinator: &mut Coordinator<'_, S>) -> Result<NavigationResult, Error> {
///         // Send initial view via StatefulView
///         self.view.send().await?;
///         
///         // Listen for user interaction
///         let (action, _) = self.view.listen_once().await?;
///         
///         // Return navigation result
///         Ok(NavigationResult::Back)
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait Controller<S>: Send + Sync {
    /// Runs this controller until completion.
    ///
    /// This method executes the controller's interactive flow, handling
    /// user interactions until the controller decides to exit.
    ///
    /// The controller uses the provided coordinator to:
    /// - Access Discord context
    /// - Read/write coordinator state
    /// Views handle their own message lifecycle via [`StatefulView`](crate::bot::views::StatefulView).
    ///
    /// # Parameters
    ///
    /// - `coordinator`: Reference to the coordinator for context and state
    ///
    /// # Returns
    ///
    /// Returns a `NavigationResult` indicating where to navigate next,
    /// or an error if something goes wrong.
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error>;
}
