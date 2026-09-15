//! The translation layer between Discord interactions and view sessions.
//!
//! Every interactive view message belongs to exactly one live session
//! runtime at a time — a Host (TEA) session or a plugin view session — and
//! exactly one runtime acknowledges each interaction on it:
//!
//! - **Host session**: the global event handler skips the interaction
//!   entirely. The Host loop translates the interaction into the feature's
//!   messages, handles it, and acknowledges it after handling — except for
//!   modal-triggering actions, where opening the modal is itself the
//!   response, and modal submissions, which the poise modal task spawned by
//!   the feature acknowledges when they arrive. That modal-task ack holds
//!   only while the Host session is live: a submission arriving after the
//!   session ends finds no claim, so the global handler acknowledges it and
//!   routes it to the plugin engine, poise's own ack then fails with
//!   `AlreadyResponded`, and the feature's modal task swallows both the
//!   failed ack and the send on the closed channel.
//! - **Plugin session** (or no session at all): the global event handler
//!   acknowledges first — the plugin round trip can take most of Discord's
//!   three-second response window — and then routes the interaction through
//!   the plugin view engine, whose own session map answers live versus
//!   stale.
//!
//! This module owns the Host side of that routing decision
//! ([`TranslateLayer`]); the plugin engine keeps tracking its own sessions
//! ([`crate::plugin::InteractionEngine`]), so each runtime has exactly one
//! source of session truth. Host sessions are claimed only by the Host
//! loop, and plugin sessions register only the messages their own responses
//! created, so the two never overlap.
//!
//! The remaining translation directions live at their call sites for now:
//! Discord events become feature messages through the collectors and
//! [`crate::bot::gui::feature::GuiFeature::translate`], and renders become
//! Discord payloads in the Host's render step.

use std::collections::HashSet;
use std::sync::RwLock;

use poise::serenity_prelude::MessageId;

/// Tracks the messages owned by live Host (TEA) sessions.
///
/// The global event handler consults [`TranslateLayer::host_owned`] before
/// it acknowledges a component interaction or modal submission: a
/// Host-owned message is skipped there, so the Host — and only the Host —
/// answers it.
#[derive(Default)]
pub struct TranslateLayer {
    host_messages: RwLock<HashSet<MessageId>>,
}

impl TranslateLayer {
    /// Creates an empty layer.
    pub fn new() -> Self {
        Self::default()
    }

    /// True when `message_id` belongs to a live Host session: the Host owns
    /// the message's interactions and their acknowledgement, and the global
    /// event handler must skip them.
    ///
    /// Poison policy — fail open. A poisoned lock means the session that
    /// held it panicked and tore itself down, so no live Host message can
    /// be in the set; reporting not-owned routes the interaction through
    /// the plugin engine, where "no open session" degrades to the stale
    /// view path, while reporting owned would make the global handler skip
    /// an interaction nobody answers (the double-ack regression in
    /// reverse). The other two paths follow: [`Self::host_session`] panics
    /// on poison rather than run a session whose ownership it cannot
    /// record, and the release path swallows it so the `Drop` path —
    /// including during unwinding — never panics.
    pub fn host_owned(&self, message_id: MessageId) -> bool {
        self.host_messages
            .read()
            .map(|messages| messages.contains(&message_id))
            .unwrap_or(false)
    }

    /// Claims `message_id` for a Host session. The message is Host-owned
    /// until the returned [`HostSession`] drops, so ownership can never
    /// outlive the loop that claimed it.
    pub fn host_session(&self, message_id: MessageId) -> HostSession<'_> {
        self.host_messages
            .write()
            .expect("translate layer lock poisoned")
            .insert(message_id);
        HostSession {
            layer: self,
            message_id,
        }
    }

    /// Releases a claimed message. Never panics: the drop path of
    /// [`HostSession`] runs through here, including during unwinding, and
    /// a poisoned lock is swallowed (see [`TranslateLayer::host_owned`]).
    fn release(&self, message_id: MessageId) {
        if let Ok(mut messages) = self.host_messages.write() {
            messages.remove(&message_id);
        }
    }
}

/// A live Host session's claim on its message, held for the Host loop's
/// lifetime. Dropping the claim — on return, error propagation, or unwind —
/// releases the message back to plugin-session routing.
pub struct HostSession<'a> {
    layer: &'a TranslateLayer,
    message_id: MessageId,
}

impl Drop for HostSession<'_> {
    fn drop(&mut self) {
        self.layer.release(self.message_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_live_host_session_owns_its_messages_interactions() {
        let layer = TranslateLayer::new();
        let message = MessageId::new(1);

        let _session = layer.host_session(message);

        assert!(layer.host_owned(message));
    }

    #[test]
    fn dropping_the_host_session_returns_the_message_to_plugin_routing() {
        let layer = TranslateLayer::new();
        let message = MessageId::new(2);

        {
            let _session = layer.host_session(message);
        }

        assert!(!layer.host_owned(message));
    }

    #[test]
    fn a_message_without_a_host_session_is_not_host_owned() {
        let layer = TranslateLayer::new();

        assert!(!layer.host_owned(MessageId::new(3)));
    }

    #[test]
    fn a_host_session_does_not_own_other_messages() {
        let layer = TranslateLayer::new();

        let _session = layer.host_session(MessageId::new(4));

        assert!(!layer.host_owned(MessageId::new(5)));
    }

    /// The double-ack regression (live `/about` Back click): the global
    /// component handler acked every click before the Host loop acked again
    /// after handling, because its only routing input was the plugin
    /// engine's session map — which never contains Host messages. The fix
    /// routes through the layer: while the Host session is live the handler
    /// sees the message as Host-owned and skips it (the Host acks exactly
    /// once), and after the session ends the message returns to plugin
    /// routing.
    #[test]
    fn an_about_back_click_routes_to_the_host_only_while_its_session_is_live() {
        let layer = TranslateLayer::new();
        let message = MessageId::new(6);

        assert!(!layer.host_owned(message));

        let session = layer.host_session(message);
        assert!(layer.host_owned(message));

        drop(session);
        assert!(!layer.host_owned(message));
    }
}
