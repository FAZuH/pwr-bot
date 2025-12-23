use std::any::Any;
use std::any::TypeId;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::RwLock;

use anyhow::Result;
use tokio::runtime;

use crate::subscriber::Subscriber;

type AsyncSubscriber<E> =
    Box<dyn Fn(E) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;
type Subscribers = Arc<RwLock<HashMap<TypeId, Vec<Box<dyn Any + Send + Sync>>>>>;

pub struct EventBus {
    subscribers: Subscribers,
    rt: runtime::Runtime,
}

impl EventBus {
    pub fn new() -> Self {
        let rt = runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("Error spawning tokio runtime for EventBus");
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            rt,
        }
    }

    pub fn register_callback<E, F, Fut>(&self, callback: F)
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
    }

    pub fn register_subcriber<E, S>(&self, subscriber: Arc<S>)
    where
        E: 'static + Send + Sync + Clone,
        S: Subscriber<E> + Send + Sync + 'static,
    {
        self.register_callback(move |event: E| {
            let h = subscriber.clone();
            async move { h.callback(event).await }
        })
    }

    pub fn publish<E>(&self, event: E)
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
            self.rt.spawn(async move {
                futures::future::join_all(futures).await;
            });
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
