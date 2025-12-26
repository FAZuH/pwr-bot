pub mod event_bus;
pub mod feed_update_event;

/// Marker trait for events that can be dispatched through the event bus.
///
/// Automatically implemented for all types that are thread-safe and have
/// a static lifetime. The `as_any()` downcasting method is provided by
/// a blanket implementation.
pub trait Event: std::any::Any + Send + Sync + 'static {
    /// Downcast this event to a concrete type.
    ///
    /// Used internally by event handlers to extract the specific event type
    /// from a trait object. Most users won't need to call this directly.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Get the name of the event type.
    fn event_name(&self) -> String {
        std::any::type_name::<Self>().to_string()
    }
}
