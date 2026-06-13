//! Event bus for publish-subscribe communication.

use std::any::Any;
use std::any::TypeId;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::RwLock;

use anyhow::Result;

/// Trait for typed event subscribers.
#[async_trait::async_trait]
pub trait Subscriber<E> {
    /// Called when an event of type E is published.
    async fn callback(&self, event: E) -> Result<()>;
}

type AsyncSubscriber<E> =
    Box<dyn Fn(E) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;
type Subscribers = Arc<RwLock<HashMap<TypeId, Vec<Box<dyn Any + Send + Sync>>>>>;

/// A named event handler receiving JSON payloads.
type NamedHandler = Box<dyn Fn(serde_json::Value) -> Result<()> + Send + Sync>;

/// Event bus for publishing events to subscribers.
pub struct EventBus {
    subscribers: Subscribers,
    /// Name-based subscriber dispatch (for plugins).
    named_subscribers: Arc<RwLock<HashMap<String, Vec<NamedHandler>>>>,
}

impl EventBus {
    /// Creates a new event bus with no subscribers.
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            named_subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Registers a callback function for events of type E.
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

    /// Registers a subscriber that implements the Subscriber trait.
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

    /// Publishes an event to all registered subscribers.
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

    // ---- Name-based API (for plugins) ----

    /// Registers a named event handler that receives JSON payloads.
    pub fn subscribe_named(&self, event_name: &str, handler: NamedHandler) -> &Self {
        self.named_subscribers
            .write()
            .unwrap()
            .entry(event_name.to_string())
            .or_default()
            .push(handler);
        self
    }

    /// Publishes an event by name with a JSON payload.
    pub fn publish_named(&self, event_name: &str, payload: serde_json::Value) -> Result<()> {
        let subs = self.named_subscribers.read().unwrap();
        if let Some(handlers) = subs.get(event_name) {
            for handler in handlers {
                let _ = handler(payload.clone());
            }
        }
        Ok(())
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
    async fn event_bus() {
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

    #[tokio::test]
    async fn named_event_bus() {
        let bus = EventBus::new();
        let called = Arc::new(AtomicI32::new(0));
        let c = called.clone();

        bus.subscribe_named(
            "test.event",
            Box::new(move |payload| {
                let val = payload.get("val").and_then(|v| v.as_i64()).unwrap_or(0);
                c.fetch_add(val as i32, Ordering::SeqCst);
                Ok(())
            }),
        );

        bus.publish_named("test.event", serde_json::json!({"val": 5}))
            .unwrap();

        sleep(Duration::from_millis(50)).await;
        assert_eq!(called.load(Ordering::SeqCst), 5);
    }
}
