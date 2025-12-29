use std::any::Any;
use std::any::TypeId;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::RwLock;

use anyhow::Result;

use crate::subscriber::Subscriber;

type AsyncSubscriber<E> =
    Box<dyn Fn(E) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;
type Subscribers = Arc<RwLock<HashMap<TypeId, Vec<Box<dyn Any + Send + Sync>>>>>;

pub struct EventBus {
    subscribers: Subscribers,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register_callback<E, F, Fut>(&self, callback: F) -> &Self
    where
        E: 'static + Send + Sync,
        F: Fn(E) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let type_id = TypeId::of::<E>();
        let wrapped_sub: AsyncSubscriber<E> = Box::new(move |event| Box::pin(callback(event)));
        self.subscribers
            .write()
            .unwrap()
            .entry(type_id)
            .or_default()
            .push(Box::new(wrapped_sub));
        self
    }

    pub fn register_subcriber<E, S>(&self, subscriber: Arc<S>) -> &Self
    where
        E: 'static + Send + Sync + Clone,
        S: Subscriber<E> + Send + Sync + 'static,
    {
        self.register_callback(move |event: E| {
            let h = subscriber.clone();
            async move { h.callback(event).await }
        })
    }

    pub fn publish<E>(&self, event: E) -> &Self
    where
        E: 'static + Send + Sync + Clone,
    {
        let type_id = TypeId::of::<E>();
        let subs = self.subscribers.read().unwrap();
        if let Some(subs_list) = subs.get(&type_id) {
            let mut futures = Vec::new();
            for subs_box in subs_list {
                if let Some(sub) = subs_box.downcast_ref::<AsyncSubscriber<E>>() {
                    futures.push(sub(event.clone()));
                }
            }
            tokio::spawn(async move {
                futures::future::join_all(futures).await;
            });
        }
        self
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicI32;
    use std::sync::atomic::Ordering;

    use tokio::time::Duration;
    use tokio::time::sleep;

    use super::*;

    #[derive(Clone)]
    struct TestEvent {
        val: i32,
    }

    #[tokio::test]
    async fn test_event_bus() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicI32::new(0));
        let counter_clone = counter.clone();

        bus.register_callback(move |event: TestEvent| {
            let c = counter_clone.clone();
            async move {
                c.fetch_add(event.val, Ordering::SeqCst);
                Ok(())
            }
        });

        bus.publish(TestEvent { val: 10 });

        // Wait a bit for async spawn
        sleep(Duration::from_millis(50)).await;

        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }
}
