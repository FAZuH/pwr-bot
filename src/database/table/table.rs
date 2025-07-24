use async_trait::async_trait;

#[async_trait]
pub trait Table<T, ID> {
    async fn create_table(&self) -> anyhow::Result<()>;
    async fn drop_table(&self) -> anyhow::Result<()>;
    async fn select_all(&self) -> anyhow::Result<Vec<T>>;
    async fn delete_all(&self) -> anyhow::Result<()>;
    async fn insert(&self, model: &T) -> anyhow::Result<ID>;
    async fn select(&self, id: &ID) -> anyhow::Result<T>;
    async fn update(&self, model: &T) -> anyhow::Result<()>;
    async fn delete(&self, id: &ID) -> anyhow::Result<()>;
}
