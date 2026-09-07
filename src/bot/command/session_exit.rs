//! Terminal steps of a [`Router`] session: the hub handoff and the root
//! dismissal.
//!
//! A host session ends in one of three ways. Plain expiry and
//! [`Navigation::Exit`] leave the live message alone. The two steps here are
//! the ones that end it while the message still carries a Back button. Both
//! are best-effort: a failing API call is logged and the session ends
//! anyway, so a transient failure can leave a dead Back button behind.
//!
//! - **Hub handoff** ([`Navigation::SettingsMain`]): the Back button of
//!   every Back-capable host feature (about, feed settings, voice settings,
//!   welcome) exits to this target. The settings plugin renders its hub
//!   [`ViewSpec`], the live message is morphed in place into that payload,
//!   and the message id is registered with the interaction engine — the
//!   message continues its life as a plugin view session and the host run
//!   ends. The Host session claim is already released by then: it drops with
//!   the [`Host::run`] loop, before the Router pops the exit navigation.
//! - **Root dismissal** (a [`Navigation::Back`] marker on an empty stack):
//!   the message shows the root view, so Back deletes it. An ephemeral
//!   message is left alone instead — it belongs to the interaction that
//!   produced it. No host view is ephemeral today (the Host renders without
//!   the flag), so that branch is explicit rather than assumed. No feature
//!   pushes the Back marker today — every Back hands off to the hub — so
//!   this step is the walk's well-defined empty-history branch.
//!
//! The Router's session loop fetches the live message once and passes it to
//! both steps: a refetch on the handoff-failure path would be a fresh
//! failure point that skips the dismissal.
//!
//! The morph goes through the bare channel-scoped edit route (`HostIo`), the
//! same route the plugin view router uses for plugin payloads: a
//! [`ViewSpec`]'s `data` is raw Discord message JSON and
//! [`serenity::Component`] is not `Deserialize`, so the spec cannot ride a
//! typed `CreateReply`.
//!
//! [`Router`]: crate::bot::command::Router
//! [`Host::run`]: crate::bot::gui::rt::Host::run
//! [`ViewSpec`]: pwr_plugin_protocol::ViewSpec

use std::sync::Arc;

use log::warn;
use poise::ReplyHandle;
use poise::serenity_prelude as serenity;
use serde_json::json;

use crate::bot::command::Context;
use crate::bot::command::Error;
use crate::plugin::HostIo;
use crate::plugin::InteractionEngine;
use crate::plugin::RunningPlugin;
use crate::plugin::SerenityHostIo;
use crate::plugin::edit_body_for_transport;
use crate::plugin::validate_view_data;

/// The plugin whose hub view the handoff adopts the message into: the
/// settings core plugin, and the `settings` command it registers.
pub const SETTINGS_PLUGIN: &str = "settings";

/// The end-of-session action the root dismissal takes for a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dismissal {
    /// Delete the message: a public root view's Back button would otherwise
    /// stay clickable and dead.
    Delete,
    /// Leave the message alone: an ephemeral view belongs to the interaction
    /// that produced it and disappears with it.
    Leave,
}

/// Which action the root dismissal takes for `message`: public views are
/// deleted, ephemeral views are left alone.
fn dismissal_for(message: &serenity::Message) -> Dismissal {
    let ephemeral = message
        .flags
        .as_ref()
        .is_some_and(|flags| flags.contains(serenity::MessageFlags::EPHEMERAL));
    if ephemeral {
        Dismissal::Leave
    } else {
        Dismissal::Delete
    }
}

/// Ends the session on a root Back, or on a failed hub handoff: deletes the
/// public view so no dead Back button stays behind, and leaves an ephemeral
/// one alone.
///
/// `message` is the reply's message, fetched by the caller: the
/// handoff-failure path already holds it, and fetching it again here would
/// add a failure point that skips the dismissal.
///
/// Best-effort: a message the user deleted by hand (or any failing API call)
/// is logged and the session ends anyway — the alternative is answering a
/// view the user already dismissed with a command error.
pub async fn dismiss_root_view(
    ctx: &Context<'_>,
    reply: &ReplyHandle<'_>,
    message: &serenity::Message,
) {
    if dismissal_for(message) == Dismissal::Leave {
        return;
    }
    if let Err(error) = reply.delete(*ctx).await {
        let message_id = message.id;
        warn!("root back failed to delete message {message_id}: {error}");
    }
}

/// Hands the live view message to the settings plugin's hub: resolves the
/// running plugin, then adopts the message into its hub view (see
/// [`adopt_message_into_hub`]).
///
/// Fails when the plugin is not running, the invoke fails, the Gate rejects
/// the payload, or the edit fails — the caller decides what a failed handoff
/// means for the session.
pub async fn handoff_to_hub(ctx: &Context<'_>, message: &serenity::Message) -> Result<(), Error> {
    let data = ctx.data();
    let Some(plugin) = data.plugin_manager.get(SETTINGS_PLUGIN).await else {
        return Err(Error::from(format!(
            "the `{SETTINGS_PLUGIN}` plugin is not running"
        )));
    };
    let io = SerenityHostIo::new(ctx.serenity_context().http.clone());
    adopt_message_into_hub(
        &data.plugin_engine,
        plugin,
        &io,
        serenity::ChannelId::new(message.channel_id.get()),
        message.id,
    )
    .await
}

/// The adoption half of the handoff, at the seam tests can drive offline:
/// invokes the plugin command for its [`ViewSpec`], sends the raw payload
/// through the Gate, morphs the message into it through the Discord-edit
/// seam, and only then registers the engine session on the message id.
///
/// The order is the failure story: a payload the Gate rejects, or an edit
/// that fails, leaves the message untouched and the engine sessionless, so
/// clicks on a message that never became the hub are stale-dropped rather
/// than routed to a session whose view the user cannot see.
///
/// [`ViewSpec`]: pwr_plugin_protocol::ViewSpec
pub async fn adopt_message_into_hub(
    engine: &InteractionEngine<RunningPlugin>,
    plugin: Arc<RunningPlugin>,
    io: &dyn HostIo,
    channel_id: serenity::ChannelId,
    message_id: serenity::MessageId,
) -> Result<(), Error> {
    let spec = engine
        .invoke(plugin.clone(), SETTINGS_PLUGIN, json!({}))
        .await?;
    validate_view_data(&spec.data)?;
    io.edit_message(
        channel_id.get(),
        message_id.get(),
        edit_body_for_transport(&spec.data),
        Vec::new(),
    )
    .await?;
    engine
        .register(message_id, plugin, SETTINGS_PLUGIN, spec)
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message_with(flags: Option<serenity::MessageFlags>) -> serenity::Message {
        let mut message = serenity::Message::default();
        message.flags = flags;
        message
    }

    #[test]
    fn root_back_deletes_a_view_without_flags() {
        let message = message_with(None);
        assert_eq!(dismissal_for(&message), Dismissal::Delete);
    }

    #[test]
    fn root_back_deletes_a_public_view() {
        let message = message_with(Some(serenity::MessageFlags::empty()));
        assert_eq!(dismissal_for(&message), Dismissal::Delete);
    }

    #[test]
    fn root_back_leaves_an_ephemeral_view_alone() {
        let message = message_with(Some(serenity::MessageFlags::EPHEMERAL));
        assert_eq!(dismissal_for(&message), Dismissal::Leave);
    }
}
