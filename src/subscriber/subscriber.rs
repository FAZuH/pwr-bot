use anyhow::Result;

#[async_trait::async_trait]
pub trait Subscriber<E> {
    async fn callback(&self, event: E) -> Result<()> ;
}
