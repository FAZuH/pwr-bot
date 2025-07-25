use std::any::{Any, TypeId};
use std::collections::HashMap;

use anyhow::{Ok, Result};

use super::event::Event;

pub struct EventBus {
    subscribers: HashMap<TypeId, Vec<Box<dyn Fn(&dyn Any) + Send + Sync>>>,
}

impl EventBus {
    pub fn new() -> Self {
        EventBus {
            subscribers: HashMap::new(),
        }
    }

    pub fn register<T: Event>(&mut self, callback: impl Fn(&T) + Send + Sync + 'static) {
        let callb = Box::new(move |event: &dyn Any| {
            if let Some(e) = event.downcast_ref::<T>() {
                callback(e);
            }
        });
        self.subscribers
            .entry(TypeId::of::<T>())
            .or_default()
            .push(callb);
    }

    pub async fn publish(&self, event: &dyn Event) -> Result<()> {
        let event_any = event.as_any();
        if let Some(callbacks) = self.subscribers.get(&event_any.type_id()) {
            for callback in callbacks {
                callback(event_any);
            }
        }
        Ok(())
    }
}
