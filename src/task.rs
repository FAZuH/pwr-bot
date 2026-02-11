//! Background tasks for feed polling and voice tracking.

pub mod series_feed_publisher;
pub mod voice_heartbeat;

// use std::borrow::Cow;
// use std::sync::Arc;
// use std::sync::atomic::AtomicBool;
// use std::sync::atomic::Ordering;
//
//
// use serenity::async_trait;
// use tokio::time::Duration;
// use log::info;
// use log::error;
//
// pub struct TaskBase {
//     pub name: &'static str,
//     running: AtomicBool,
//     interval: Duration,
// }
//
// impl TaskBase {
//     pub fn new(interval: Duration, name: impl for<'a> Into<Cow<'a, &'a str>>) -> Self {
//         let name = &name.into();
//         info!("Initializing {name} with interval {interval:?}");
//         Self {
//             name,
//             running: AtomicBool::new(false),
//             interval,
//         }
//     }
//
//     pub fn start<T: Task + 'static + ?Sized>(self: Arc<Self>, task: Arc<T>) -> anyhow::Result<()> {
//         if !self.running.load(Ordering::SeqCst) {
//             self.running.store(true, Ordering::SeqCst);
//             info!("Starting {} check loop.", self.name);
//             self.spawn_check_loop(task);
//         }
//         Ok(())
//     }
//
//     fn spawn_check_loop<T: Task + 'static + ?Sized>(self: Arc<Self>, task: Arc<T>) {
//         let mut interval = tokio::time::interval(self.interval);
//         tokio::spawn(async move {
//             loop {
//                 interval.tick().await;
//                 if !self.running.load(Ordering::SeqCst) {
//                     info!("Stopping check loop.");
//                     break;
//                 }
//                 if let Err(e) = task.clone().run().await {
//                     error!("Error checking updates: {}", e);
//                 }
//             }
//         });
//     }
//     pub fn stop(self: Arc<Self>) -> anyhow::Result<()> {
//         info!("Stopping {} check loop.", self.name);
//         self.running.store(false, Ordering::SeqCst);
//         Ok(())
//     }
// }
//
// #[async_trait]
// pub trait Task<T = ()>: Send + Sync {
//     async fn run(&self) -> anyhow::Result<T>;
//     fn get_base(&self) -> Arc<TaskBase>;
//
//     async fn start(self: Arc<Self>) -> anyhow::Result<()>
//     where
//         Self: Task + 'static
//     {
//         <Self as Task<T>>::get_base(&self).start(self)
//     }
//
//     async fn stop(self: Arc<Self>) -> anyhow::Result<()> {
//         self.get_base().stop()
//     }
// }
