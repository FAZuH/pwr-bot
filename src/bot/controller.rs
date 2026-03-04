//! Controller trait for the MVC-C pattern.

use crate::bot::commands::Error;
use crate::bot::coordinator::Coordinator;

/// Trait for command controllers that manage state and view lifecycle.
///
/// In the MVC-C pattern, a `Controller` is responsible for:
/// 1. Fetching initial data from services.
/// 2. Constructing and running a [`ViewEngine`](crate::bot::views::ViewEngine).
/// 3. Processing view actions and calling services.
/// 4. Deciding where to navigate next by returning a [`NavigationResult`](crate::bot::navigation::NavigationResult).
#[async_trait::async_trait]
pub trait Controller<S>: Send + Sync {
    /// Executes the controller logic.
    ///
    /// The `coordinator` provides access to shared state and navigation.
    async fn run(&mut self, coordinator: std::sync::Arc<Coordinator<'_, S>>) -> Result<(), Error>;
}
