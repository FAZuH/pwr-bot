use async_trait::async_trait;

#[async_trait]
pub trait Listener: Send + Sync {
    async fn subscribe(&mut self, user_id: String, series_id: String, series_type: String, wehook_url: String) -> anyhow::Result<()>;
    async fn unsubscribe(&mut self, user_id: String, series_id: String) -> anyhow::Result<()>;
    fn start(&mut self) -> anyhow::Result<()>;
    fn stop(&mut self) -> anyhow::Result<()>;
}
