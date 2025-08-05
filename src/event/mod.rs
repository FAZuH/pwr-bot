pub mod event_bus;
pub mod series_update_event;

pub trait Event: std::any::Any + Send + Sync + 'static {
    fn as_any(&self) -> &dyn std::any::Any;
}
