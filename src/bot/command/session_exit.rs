//! Terminal steps of a [`Router`] session: the Settings section handoff and
//! the root dismissal.
//!
//! A host session ends in one of three ways. Plain expiry and
//! [`Navigation::Exit`] leave the live message alone. The two steps here are
//! the ones that end it while the message still carries a button. Both are
//! best-effort: a failing API call is logged and the session ends anyway, so
//! a transient failure can leave a dead button behind.
//!
//! - **Settings section handoff** ([`Navigation::SettingsSection`]): a
//!   section tile on the Settings view was pressed. The target plugin's
//!   command renders its panel [`ViewSpec`], the live message is morphed in
//!   place into that payload, and the message id is registered with the
//!   interaction engine — the message continues its life as a plugin view
//!   session and the host run ends. The Host session claim is already
//!   released by then: it drops with the [`Host::run`] loop, before the
//!   Router pops the exit navigation.
//! - **Root dismissal** (a [`Navigation::Back`] marker on an empty stack):
//!   the message shows the root view — the Settings view, whose Back
//!   is the Root Back — so Back deletes it. An ephemeral message is left
//!   alone instead: it belongs to the interaction that produced it. The
//!   Host renders without the ephemeral flag, so every root view deletes.
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
//!
//! [`Navigation::Exit`]: crate::bot::navigation::Navigation::Exit
//! [`Navigation::SettingsSection`]: crate::bot::navigation::Navigation::SettingsSection
//! [`Navigation::Back`]: crate::bot::navigation::Navigation::Back

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
use crate::plugin::command::ACTOR_CONTEXT_KEY;
use crate::plugin::command::actor_context_with_guild_name;
use crate::plugin::decode_runtime_files_with_existing;
use crate::plugin::edit_body_for_transport;
use crate::plugin::validate_view_spec;

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

/// Ends the session on a root Back, or on a failed section handoff: deletes
/// the public view so no dead button stays behind, and leaves an ephemeral
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

/// Hands the live view message to a settings section's panel: resolves the
/// running plugin, then adopts the message into the view its command
/// renders (see [`adopt_message_into_section`]). The invoke args carry the
/// invocation's guild and actor context when the interaction ran in a guild —
/// the panel plugins key their settings by guild and authorize writes.
///
/// Fails when the plugin is not running, the invoke fails, the Gate rejects
/// the payload, or the edit fails — the caller decides what a failed handoff
/// means for the session.
pub async fn handoff_to_section(
    ctx: &Context<'_>,
    plugin: &str,
    command: &str,
    message: &serenity::Message,
) -> Result<(), Error> {
    let data = ctx.data();
    let Some(running) = data.plugin_manager.get(plugin).await else {
        return Err(Error::from(format!("the `{plugin}` plugin is not running")));
    };
    let mut args = json!({});
    if let Some(guild_id) = ctx.guild_id() {
        args["guild_id"] = json!(guild_id.get());
    }
    if let poise::Context::Application(app) = ctx {
        let guild_name = ctx.guild().map(|guild| guild.name.to_string());
        args[ACTOR_CONTEXT_KEY] =
            actor_context_with_guild_name(app.interaction, guild_name.as_deref());
    }
    let io = SerenityHostIo::new(ctx.serenity_context().http.clone());
    adopt_message_into_section(
        &data.plugin_engine,
        running,
        command,
        args,
        &io,
        Some(data.previews.as_ref()),
        message,
    )
    .await
}

/// The adoption half of the handoff, at the seam tests can drive offline:
/// invokes the plugin command for its [`ViewSpec`], sends the raw payload
/// through the Gate, resolves the body's declared attachment slots like
/// every other transport (ADR-0012 — never a dangling slot), morphs the
/// message into it through the Discord-edit seam, and only then registers
/// the engine session on the message id.
///
/// The order is the failure story: a payload the Gate rejects, or an edit
/// that fails, leaves the message untouched and the engine sessionless, so
/// clicks on a message that never became the panel are stale-dropped rather
/// than routed to a session whose view the user cannot see.
///
/// [`ViewSpec`]: pwr_plugin_protocol::ViewSpec
pub async fn adopt_message_into_section(
    engine: &InteractionEngine<RunningPlugin>,
    plugin: Arc<RunningPlugin>,
    command: &str,
    args: serde_json::Value,
    io: &dyn HostIo,
    previews: Option<&crate::plugin::preview::PreviewResolver>,
    message: &serenity::Message,
) -> Result<(), Error> {
    let channel_id = serenity::ChannelId::new(message.channel_id.get());
    let message_id = message.id;
    let guild_id = args.get("guild_id").and_then(serde_json::Value::as_u64);
    let author_id = crate::plugin::interaction::author_id_from_payload(&args)
        .ok_or_else(|| Error::from("settings handoff is missing its interaction author"))?;
    let spec = engine.invoke(plugin.clone(), command, args).await?;
    validate_view_spec(&spec).map_err(|error| Error::from(error.msg))?;
    let (body, mut attachments) = match previews {
        Some(previews) => {
            previews
                .resolve(edit_body_for_transport(&spec.data), guild_id)
                .await
        }
        None => (edit_body_for_transport(&spec.data), Vec::new()),
    };
    attachments.extend(
        decode_runtime_files_with_existing(&spec.files, attachments.len())
            .map_err(|error| Error::from(error.msg))?,
    );
    io.edit_message(channel_id.get(), message_id.get(), body, attachments)
        .await?;
    engine
        .register(message_id, author_id, plugin, command, spec)
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
