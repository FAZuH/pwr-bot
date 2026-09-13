//! The shared lifecycle vocabulary for Host-driven views.
//!
//! Every Host-driven feature speaks the same two lifecycle moments: the host
//! dispatches [`Lifecycle::Start`] on boot and [`Lifecycle::Expired`] when the
//! view loop times out. Each feature's message enum carries them as one
//! wrapped [`Lifecycle`] variant (and converts with `From<Lifecycle>`), and
//! [`Lifecycle::handle`] is the one common handler: start never produces
//! effects, expiry runs the feature's own expiry behavior. The per-view
//! choice — no-op versus persist-on-exit — lives at that single call site, so
//! lifecycle semantics cannot drift between views.

/// The lifecycle moments every Host-driven view shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// Boot handshake — the host dispatches this on start.
    Start,
    /// The view loop timed out.
    Expired,
}

impl Lifecycle {
    /// The one common lifecycle handler.
    ///
    /// [`Lifecycle::Start`] is always a no-op — the boot cycle only renders
    /// the initial model. [`Lifecycle::Expired`] runs `on_expired`, the
    /// feature's expiry behavior: persist-on-exit views return their persist
    /// effect there, the rest return none.
    pub fn handle<E>(self, on_expired: impl FnOnce() -> Vec<E>) -> Vec<E> {
        match self {
            Lifecycle::Start => Vec::new(),
            Lifecycle::Expired => on_expired(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_never_runs_the_expiry_behavior() {
        let effects: Vec<u8> = Lifecycle::Start.handle(|| vec![7]);
        assert!(effects.is_empty());
    }

    #[test]
    fn expired_runs_the_expiry_behavior() {
        let effects: Vec<u8> = Lifecycle::Expired.handle(|| vec![7]);
        assert_eq!(effects, vec![7]);
    }
}
