//! Helper functions for driving views in automated tests.

use std::sync::Arc;

use crate::bot::command::Context;
use crate::bot::command::Error;
use crate::bot::command::prelude::Router;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::view::Action;
use crate::bot::view::ActionRegistry;
use crate::bot::view::SelectValues;
use crate::bot::view::SyntheticEvent;
use crate::bot::view::ViewCmd;
use crate::bot::view::ViewContext;
use crate::bot::view::ViewEvent;
use crate::bot::view::ViewHandler;
use crate::bot::view::ViewRender;
use crate::bot::view::ViewSender;

struct NoopSender<T>(std::marker::PhantomData<T>);

impl<T: Action> ViewSender<T> for NoopSender<T> {
    fn send(&self, _message: (Option<T>, ViewEvent)) {}
}

/// Returns a no-op sender for tests that do not need to dispatch follow-up events.
pub fn noop_sender<T: Action + 'static>() -> Arc<dyn ViewSender<T>> {
    Arc::new(NoopSender(std::marker::PhantomData))
}

/// Renders a view and returns the populated action registry.
///
/// The rendered response is discarded to avoid lifetime complications.
pub fn extract_actions<T, H>(handler: &H) -> ActionRegistry<T>
where
    H: ViewHandler<Action = T> + ViewRender<Action = T>,
    T: Action,
{
    let mut registry = ActionRegistry::new();
    let _ = handler.render(&mut registry);
    registry
}

/// Simulates a button click on the given handler.
pub async fn simulate_click<'a, T, H>(
    ctx: Context<'a>,
    handler: &mut H,
    action: T,
    coordinator: Arc<Router<'a>>,
) -> Result<ViewCmd, Error>
where
    H: ViewHandler<Action = T>,
    T: Action + 'static,
{
    let view_ctx = ViewContext {
        poise: ctx,
        action: Some(action),
        event: ViewEvent::Synthetic(SyntheticEvent::Button),
        tx: noop_sender(),
        coordinator,
    };
    handler.handle(view_ctx).await
}

/// Simulates a select-menu choice on the given handler.
pub async fn simulate_select<'a, T, H>(
    ctx: Context<'a>,
    handler: &mut H,
    action: T,
    values: SelectValues,
    coordinator: Arc<Router<'a>>,
) -> Result<ViewCmd, Error>
where
    H: ViewHandler<Action = T>,
    T: Action + 'static,
{
    let view_ctx = ViewContext {
        poise: ctx,
        action: Some(action),
        event: ViewEvent::Synthetic(SyntheticEvent::Select(values)),
        tx: noop_sender(),
        coordinator,
    };
    handler.handle(view_ctx).await
}

/// Renders a [`GuiFeature`]'s view and returns the populated action registry.
pub fn feature_actions<F: GuiFeature>(model: &F::Model) -> ActionRegistry<F::Action> {
    let mut registry = ActionRegistry::new();
    let _ = F::view(model, &mut registry);
    registry
}

/// Translates a fired action into the [`GuiFeature`] message it means,
/// using empty select values.
pub fn translate_feature_action<F: GuiFeature>(
    action: &F::Action,
    model: &F::Model,
) -> Option<F::Msg> {
    F::translate(action, SelectValues::String(Vec::new()), model)
}

/// Feeds a [`GuiFeature`] message into its pure `update`, returning the
/// effects it produced.
pub fn apply_feature_msg<F: GuiFeature>(msg: F::Msg, model: &mut F::Model) -> Vec<F::Effect> {
    F::update(msg, model)
}
