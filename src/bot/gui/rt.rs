//! The TEA host runtime driving one [`GuiFeature`] at a time.
//!
//! [`Host`] owns the event loop: it builds the model, drives the first update
//! via the feature's start message, starts the collectors, and then loops on
//! `{ await event → translate → update → execute effects → view → send }`
//! until the feature signals termination or the loop times out.

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use poise::CreateReply;
use poise::serenity_prelude::*;
use tokio::sync::RwLock;
use tokio::sync::mpsc;

use crate::bot::command::Context;
use crate::bot::command::Error;
use crate::bot::command::prelude::Router;
use crate::bot::gui::effects::EffectHandler;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::view::ActionRegistry;
use crate::bot::view::SelectValues;
use crate::bot::view::SyntheticEvent;
use crate::bot::view::ViewChannel;
use crate::bot::view::ViewEvent;

type Registry<T> = Arc<RwLock<ActionRegistry<T>>>;

/// The TEA host for a single command feature.
pub struct Host<'a, F, H>
where
    F: GuiFeature,
    H: EffectHandler<Effect = F::Effect, Msg = F::Msg>,
{
    ctx: Context<'a>,
    model: F::Model,
    handler: H,
    timeout: Duration,
    coordinator: Arc<Router<'a>>,
    _marker: PhantomData<F>,
}

impl<'a, F, H> Host<'a, F, H>
where
    F: GuiFeature + 'static,
    F::Action: 'static,
    H: EffectHandler<Effect = F::Effect, Msg = F::Msg> + 'static,
{
    /// Creates a host for the feature, building its model from `config`.
    pub fn new(
        ctx: Context<'a>,
        config: F::Config,
        handler: H,
        timeout: Duration,
        coordinator: Arc<Router<'a>>,
    ) -> Self {
        Self {
            ctx,
            model: F::initial(config),
            handler,
            timeout,
            coordinator,
            _marker: PhantomData,
        }
    }

    /// Runs the interactive loop until the feature terminates or times out.
    pub async fn run(&mut self) -> Result<(), Error> {
        let registry: Registry<F::Action> = Arc::new(RwLock::new(ActionRegistry::new()));
        let mut channel = ViewChannel::new(F::channel_config(), registry.clone());
        let (fx_tx, mut fx_rx) = mpsc::unbounded_channel::<F::Msg>();

        // Boot: drive the first update/render through the start message.
        self.apply(F::start_msg(), &fx_tx);
        self.render_view(&registry).await?;

        let msg_id = {
            let lock = self.coordinator.reply_handle().await;
            lock.as_ref()
                .expect("reply_handle must exist after the initial render")
                .message()
                .await?
                .id
        };

        channel.start(
            &self.ctx,
            msg_id,
            self.ctx.author().id,
            self.ctx.channel_id(),
            self.timeout,
        );

        loop {
            tokio::select! {
                maybe_msg = fx_rx.recv() => {
                    let Some(msg) = maybe_msg else { break; };
                    self.apply(msg, &fx_tx);
                    self.render_view(&registry).await?;
                }
                maybe_event = channel.recv() => {
                    let Some((action, event)) = maybe_event else { break; };
                    match &event {
                        ViewEvent::Timeout => {
                            let msg = F::timeout_msg();
                            let nav = F::exit_navigation(&msg);
                            self.apply(msg, &fx_tx);
                            if let Some(nav) = nav {
                                self.coordinator.navigate(nav).await;
                            }
                            break;
                        }
                        ViewEvent::Component(interaction) => {
                            let Some(action) = action else {
                                interaction
                                    .create_response(
                                        self.ctx.http(),
                                        CreateInteractionResponse::Acknowledge,
                                    )
                                    .await
                                    .ok();
                                continue;
                            };
                            // Modal triggers consume the interaction (the
                            // already-responded equivalent): opening
                            // the modal already responds to it, so the host
                            // skips the acknowledge and the re-render. The
                            // modal submission arrives later as a `Msg`.
                            if F::open_modal(
                                &action,
                                self.ctx.serenity_context().clone(),
                                interaction.clone(),
                                fx_tx.clone(),
                            ) {
                                continue;
                            }
                            let values = select_values(&event)
                                .unwrap_or(SelectValues::String(Vec::new()));
                            let Some(msg) = F::translate(&action, values, &self.model) else {
                                interaction
                                    .create_response(
                                        self.ctx.http(),
                                        CreateInteractionResponse::Acknowledge,
                                    )
                                    .await
                                    .ok();
                                continue;
                            };
                            let nav = F::exit_navigation(&msg);
                            self.apply(msg, &fx_tx);
                            // Acknowledge after handling, mirroring the old engine.
                            let raw = interaction.clone();
                            raw.create_response(
                                self.ctx.http(),
                                CreateInteractionResponse::Acknowledge,
                            )
                            .await
                            .ok();
                            if let Some(nav) = nav {
                                self.coordinator.navigate(nav).await;
                                break;
                            }
                            self.render_view(&registry).await?;
                        }
                        other => {
                            let Some(msg) = F::on_event(other, &self.model) else {
                                continue;
                            };
                            let nav = F::exit_navigation(&msg);
                            self.apply(msg, &fx_tx);
                            if let Some(nav) = nav {
                                self.coordinator.navigate(nav).await;
                                break;
                            }
                            self.render_view(&registry).await?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Applies a message to the model and executes the returned effects.
    ///
    /// Effects execute synchronously through the handler; the messages the
    /// handler returns are pushed onto the host's message channel so they are
    /// processed by the next loop iteration. Async effects deliver their
    /// follow-up messages directly on the same channel (via the `tx` sender).
    fn apply(&mut self, msg: F::Msg, fx_tx: &mpsc::UnboundedSender<F::Msg>) {
        let effects = F::update(msg, &mut self.model);
        for effect in effects {
            let followups = self.handler.execute(effect, fx_tx.clone());
            for followup in followups {
                let _ = fx_tx.send(followup);
            }
        }
    }

    /// Renders the current model and edits the live message (or sends the
    /// first one).
    async fn render_view(&self, registry: &Registry<F::Action>) -> Result<(), Error> {
        let mut reg = registry.write().await;
        reg.clear();
        let components = F::view(&self.model, &mut reg);
        let attachments = F::attachments(&self.model);
        let mut reply = CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(components);
        for attachment in attachments {
            reply = reply.attachment(attachment);
        }

        let existing = { self.coordinator.reply_handle().await.as_ref().cloned() };

        if let Some(handle) = existing {
            handle.edit(self.ctx, reply).await?;
        } else {
            let handle = self.ctx.send(reply).await?;
            self.coordinator.set_reply_handle(handle).await;
        }

        Ok(())
    }
}

/// Extracts select-menu values from a component (or synthetic) event.
fn select_values(event: &ViewEvent) -> Option<SelectValues> {
    use ComponentInteractionDataKind::*;
    match event {
        ViewEvent::Component(interaction) => match &interaction.data.kind {
            StringSelect { values } => Some(SelectValues::String(values.to_vec())),
            ChannelSelect { values } => Some(SelectValues::Channel(
                values.iter().copied().map(GenericChannelId::from).collect(),
            )),
            RoleSelect { values } => Some(SelectValues::Role(values.to_vec())),
            UserSelect { values } => Some(SelectValues::User(values.to_vec())),
            _ => None,
        },
        ViewEvent::Synthetic(SyntheticEvent::Select(values)) => Some(values.clone()),
        _ => None,
    }
}
