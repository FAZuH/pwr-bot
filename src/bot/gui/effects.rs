//! The [`EffectHandler`] port and the effect-execution plumbing.
//!
//! The core returns effects as data; the shell host executes them through a
//! [`EffectHandler`] adapter. This module fixes the seam: one effect maps to
//! one real-world action (a DB query, a save, an image render, a send), and
//! results come back as follow-up `Msg`s. Adapters for the actual services
//! arrive with the features that need them; [`NoopEffectHandler`] covers the
//! no-effect case (e.g. `/about`).

use std::marker::PhantomData;

/// Port: executes one effect, returning the follow-up messages it produces.
///
/// Fast effects may run synchronously inside [`execute`](Self::execute) and
/// return their results immediately. Slow effects must `tokio::spawn` the work
/// and deliver the outcome back through the host's message channel, returning
/// no immediate messages.
pub trait EffectHandler {
    /// The effect vocabulary this handler executes.
    type Effect;
    /// The message vocabulary results are returned as.
    type Msg;

    /// Executes one effect. Returned messages are fed back into the host loop.
    fn execute(&mut self, effect: Self::Effect) -> Vec<Self::Msg>;
}

/// An [`EffectHandler`] that performs no work and returns no messages.
///
/// Used by features whose effect vocabulary is empty, and by tests driving the
/// pure core with a no-op adapter.
pub struct NoopEffectHandler<E, M>(PhantomData<(E, M)>);

impl<E, M> NoopEffectHandler<E, M> {
    /// Creates a no-op effect handler.
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<E, M> Default for NoopEffectHandler<E, M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E, M> EffectHandler for NoopEffectHandler<E, M> {
    type Effect = E;
    type Msg = M;

    fn execute(&mut self, _effect: E) -> Vec<M> {
        Vec::new()
    }
}
