use crate::bot::commands::Error;
use crate::bot::coordinator::Coordinator;

#[async_trait::async_trait]
pub trait Controller<S>: Send + Sync {
    async fn run(&mut self, coordinator: std::sync::Arc<Coordinator<'_, S>>) -> Result<(), Error>;
}
