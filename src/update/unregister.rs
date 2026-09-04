//! Pure update logic for the `/unregister` command.
//!
//! Holds the single source of truth for the command unregistration status view
//! (`UnregisterModel`) and the exhaustive message vocabulary
//! (`UnregisterMsg`). Command unregistration is a one-shot shell concern (the
//! handler performs the actual `guild_id.set_commands` call); this core tracks
//! the status and renders it.

use crate::update::lifecycle::Lifecycle;

/// The command unregistration status view model — the single source of truth.
#[derive(Debug, Clone)]
pub struct UnregisterModel {
    /// Whether unregistration is complete.
    pub is_complete: bool,
    /// Time taken in milliseconds (if complete).
    pub duration_ms: Option<u64>,
}

impl UnregisterModel {
    /// Constructs the initial unregistration model.
    pub fn new() -> Self {
        Self {
            is_complete: false,
            duration_ms: None,
        }
    }
}

impl Default for UnregisterModel {
    fn default() -> Self {
        Self::new()
    }
}

/// Messages that drive the unregistration view.
///
/// Exhaustive: every way the world can change the unregistration model is one
/// variant. The lifecycle moments share the wrapped [`Lifecycle`] form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnregisterMsg {
    /// The shared boot/timeout lifecycle moments.
    Lifecycle(Lifecycle),
    /// Command unregistration finished.
    Unregistered { duration_ms: u64 },
}

impl From<Lifecycle> for UnregisterMsg {
    fn from(lifecycle: Lifecycle) -> Self {
        Self::Lifecycle(lifecycle)
    }
}

/// Effects the unregistration view can request.
///
/// Empty: command unregistration is a one-shot shell concern executed by the
/// command handler; this core only tracks and renders the status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnregisterEffect {}

/// The pure update function — the only writer of the model.
pub fn update(msg: UnregisterMsg, model: &mut UnregisterModel) -> Vec<UnregisterEffect> {
    match msg {
        UnregisterMsg::Unregistered { duration_ms } => {
            model.is_complete = true;
            model.duration_ms = Some(duration_ms);
            Vec::new()
        }
        UnregisterMsg::Lifecycle(lifecycle) => lifecycle.handle(Vec::new),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_keeps_model_incomplete() {
        let mut m = UnregisterModel::new();
        let effects = update(UnregisterMsg::Lifecycle(Lifecycle::Start), &mut m);
        assert!(effects.is_empty());
        assert!(!m.is_complete);
        assert_eq!(m.duration_ms, None);
    }

    #[test]
    fn unregistered_marks_complete_with_duration() {
        let mut m = UnregisterModel::new();
        let effects = update(UnregisterMsg::Unregistered { duration_ms: 456 }, &mut m);
        assert!(effects.is_empty());
        assert!(m.is_complete);
        assert_eq!(m.duration_ms, Some(456));
    }
}
