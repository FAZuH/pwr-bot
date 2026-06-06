//! Controller trait

use crate::bot::command::Error;
use crate::bot::coordinator::Router;

#[async_trait::async_trait]
pub trait CommandHandler: Send + Sync {
    /// Executes the controller logic.
    ///
    /// The `coordinator` provides access to shared state and navigation.
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error>;
}
