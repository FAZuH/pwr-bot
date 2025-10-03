use pwr_bot::event::event_bus::EventBus;
use pwr_bot::subscriber::Subscriber;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
struct TestEvent {
    data: u32,
}

impl pwr_bot::event::Event for TestEvent {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct TestSubscriber {
    counter: Arc<AtomicU32>,
}

#[async_trait::async_trait]
impl Subscriber<TestEvent> for TestSubscriber {
    async fn callback(&self, event: TestEvent) -> anyhow::Result<()> {
        self.counter.fetch_add(event.data, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn test_event_bus_register_and_publish() {
    let event_bus = EventBus::new();
    let counter = Arc::new(AtomicU32::new(0));

    let counter_clone = counter.clone();
    event_bus.register_callback(move |event: TestEvent| {
        let counter_clone = counter_clone.clone();
        async move {
            counter_clone.fetch_add(event.data, Ordering::SeqCst);
            Ok(())
        }
    });

    event_bus.publish(TestEvent { data: 5 });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    assert_eq!(counter.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn test_event_bus_subscriber_trait() {
    let event_bus = EventBus::new();
    let counter = Arc::new(AtomicU32::new(0));
    let subscriber = Arc::new(TestSubscriber { counter: counter.clone() });

    event_bus.register_subcriber(subscriber);

    event_bus.publish(TestEvent { data: 10 });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    assert_eq!(counter.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_event_bus_no_subscribers() {
    let event_bus = EventBus::new();
    // No subscribers registered
    event_bus.publish(TestEvent { data: 10 });
    // Nothing should happen, no panic
}

#[tokio::test]
async fn test_event_bus_multiple_subscribers() {
    let event_bus = EventBus::new();
    let counter1 = Arc::new(AtomicU32::new(0));
    let counter2 = Arc::new(AtomicU32::new(0));

    let s1 = Arc::new(TestSubscriber { counter: counter1.clone() });
    let s2 = Arc::new(TestSubscriber { counter: counter2.clone() });

    event_bus.register_subcriber(s1);
    event_bus.register_subcriber(s2);

    event_bus.publish(TestEvent { data: 7 });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    assert_eq!(counter1.load(Ordering::SeqCst), 7);
    assert_eq!(counter2.load(Ordering::SeqCst), 7);
}
