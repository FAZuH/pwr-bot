//! Event system for pub/sub communication between components.

pub mod event_bus;

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

/// Event emitted by a plugin to the host, broadcast on the event bus so host
/// subscribers can react instead of the event being dropped.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PluginEvent {
    /// The plugin that emitted the event.
    pub plugin: String,
    /// Event name, e.g. `settings.saved`.
    pub name: String,
    /// Opaque event payload.
    pub data: Option<serde_json::Value>,
}

impl Event for PluginEvent {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
