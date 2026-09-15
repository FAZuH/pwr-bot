//! Error handling for Discord bot commands.

use std::sync::atomic::Ordering;

use log::error;
use poise::FrameworkError;
use poise::serenity_prelude::*;

use crate::bot::Data;
use crate::bot::Error;
use crate::bot::error::BotError;
use crate::bot::reply::text_reply;
use crate::error::AppError;
use crate::service::error::ServiceError;

/// Handles framework errors and sends appropriate responses to users.
pub struct ErrorHandler;

impl ErrorHandler {
    /// Handles a framework error by classifying and responding appropriately.
    pub async fn handle(error: FrameworkError<'_, Data, Error>) {
        match error {
            FrameworkError::Command { error, ctx, .. } => {
                let (title, description) = Self::classify_error(&error, &ctx);
                let message = format!(
                    "### {}\n\n**Command:** `{}`\n**Error:** {}",
                    title,
                    ctx.command().qualified_name,
                    description
                );
                Self::send_component(&ctx, &message).await;
            }
            FrameworkError::ArgumentParse { error, ctx, .. } => {
                let message = format!(
                    "### ⚠️ Invalid Arguments\n\n**Command:** `/{}`\n**Issue:** {}\n\n> Use `/help {}` for usage information.",
                    ctx.command().name,
                    error,
                    ctx.command().name
                );
                Self::send_component(&ctx, &message).await;
            }
            error => {
                if let Err(e) = poise::builtins::on_error(error).await {
                    error!("Error while handling error: {e}");
                }
            }
        }
    }

    /// Classifies an error and returns user-friendly title and description.
    fn classify_error(
        error: &Error,
        ctx: &poise::Context<'_, Data, Error>,
    ) -> (&'static str, String) {
        if let Some(bot_error) = error.downcast_ref::<BotError>() {
            ("❌ Action Failed", bot_error.to_string())
        } else if let Some(service_error) = error.downcast_ref::<ServiceError>() {
            ("❌ Service Error", service_error.to_string())
        } else {
            let ref_id = AppError::log_with_ref(error);
            error!(
                "Unexpected error in command `{}`: {:?}",
                ctx.command().name,
                error
            );
            (
                "❌ Internal Error",
                format!(
                    "An unexpected error occurred. Please contact the bot developer.\n-# Reference ID: {ref_id}"
                ),
            )
        }
    }

    /// Delivers an error message as a Components V2 container: edits the
    /// original response when the interaction already has an initial response
    /// (deferred or already replied), sends a reply otherwise. Editing keeps
    /// one message per command and preserves the initial response's
    /// ephemerality — the error lands in the same place the payload would
    /// have.
    async fn send_component(ctx: &poise::Context<'_, Data, Error>, message: &str) {
        let poise::Context::Application(app) = ctx else {
            let _ = ctx.send(text_reply(message)).await;
            return;
        };
        if app.has_sent_initial_response.load(Ordering::SeqCst) {
            let edit =
                text_reply(message).to_slash_initial_response_edit(EditInteractionResponse::new());
            if let Err(e) = app
                .interaction
                .edit_response(&app.serenity_context().http, edit)
                .await
            {
                error!("failed to edit the error response into the original response: {e}");
            }
        } else {
            let _ = ctx.send(text_reply(message)).await;
        }
    }
}
