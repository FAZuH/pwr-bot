//! Shared lifecycle vocabulary for plugin-driven views.
//!
//! Every plugin-driven feature speaks the same lifecycle moments: the view
//! loop dispatches [`Lifecycle::Start`] on boot and [`Lifecycle::Expired`]
//! when the view times out. A feature's message enum can carry them as one
//! wrapped [`Lifecycle`] variant, and [`Lifecycle::handle`] is the common
//! handler: start never produces effects, while expiry runs the feature's own
//! expiry behavior.

/// The lifecycle moments every plugin-driven view shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// The view loop's boot handshake.
    Start,
    /// The view loop timed out.
    Expired,
}

impl Lifecycle {
    /// Runs the feature's expiry behavior when this is [`Lifecycle::Expired`].
    ///
    /// [`Lifecycle::Start`] is always a no-op. The expiry callback lets each
    /// feature choose whether it persists or does nothing on timeout.
    pub fn handle<E>(self, on_expired: impl FnOnce() -> Vec<E>) -> Vec<E> {
        match self {
            Self::Start => Vec::new(),
            Self::Expired => on_expired(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_never_runs_expiry() {
        let effects: Vec<u8> = Lifecycle::Start.handle(|| vec![7]);
        assert!(effects.is_empty());
    }

    #[test]
    fn expired_runs_expiry() {
        let effects: Vec<u8> = Lifecycle::Expired.handle(|| vec![7]);
        assert_eq!(effects, vec![7]);
    }
}
