//! Helper functions for driving [`GuiFeature`]s in automated tests.
//!
//! [`GuiFeature`]: crate::bot::gui::feature::GuiFeature

use crate::bot::gui::feature::GuiFeature;
use crate::bot::view::ActionRegistry;
use crate::bot::view::SelectValues;

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
