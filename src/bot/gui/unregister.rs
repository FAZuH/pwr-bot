//! The `/unregister` feature shell — a [`GuiFeature`] over the pure unregister
//! core.
//!
//! A one-shot command view: the handler drives it directly (creating the model,
//! sending the unregistering reply, then applying the
//! [`UnregisterMsg::Unregistered`] result and editing to the complete reply).

use pwr_ext::component;

use crate::bot::command::prelude::*;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::feature::sealed;
use crate::bot::view::SelectValues;
use crate::update::unregister::UnregisterEffect;
use crate::update::unregister::UnregisterModel;
use crate::update::unregister::UnregisterMsg;
use crate::update::unregister::update as unregister_update;

/// The unregistration view has no interactive actions (it is a one-shot status
/// view, never rendered with a component registry in a live loop).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnregisterAction {}

impl crate::bot::view::Action for UnregisterAction {
    fn label(&self) -> &'static str {
        match *self {}
    }
}

/// The unregistration feature.
pub struct UnregisterFeature;

impl sealed::Sealed for UnregisterFeature {}

impl GuiFeature for UnregisterFeature {
    type Model = UnregisterModel;
    type Msg = UnregisterMsg;
    type Action = UnregisterAction;
    type Effect = UnregisterEffect;

    type Config = ();

    fn initial(_config: Self::Config) -> Self::Model {
        UnregisterModel::new()
    }

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Vec<Self::Effect> {
        unregister_update(msg, model)
    }

    fn view<'a>(
        model: &'a Self::Model,
        _registry: &mut ActionRegistry<Self::Action>,
    ) -> Vec<CreateComponent<'a>> {
        let title = if model.is_complete {
            "Command Unregistration Complete"
        } else {
            "Unregistering Commands"
        };

        let status_text = if model.is_complete {
            format!(
                "### {}\nSuccessfully unregistered all commands in {}ms",
                title,
                model.duration_ms.unwrap_or(0)
            )
        } else {
            format!("### {title}\nUnregistering all server commands...")
        };

        let container = component! {
            container {
                text_display { content: status_text }
            }
        };

        vec![CreateComponent::Container(container)]
    }

    fn translate(
        action: &Self::Action,
        _values: SelectValues,
        _model: &Self::Model,
    ) -> Option<Self::Msg> {
        match *action {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::bot::view::ActionRegistry;

    #[test]
    fn unregistration_view_incomplete_snapshot() {
        let model = UnregisterModel::new();
        let mut registry = ActionRegistry::new();
        let components = UnregisterFeature::view(&model, &mut registry);
        let value = serde_json::to_value(&components).unwrap();
        assert_eq!(
            value,
            json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "### Unregistering Commands\nUnregistering all server commands..."
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn unregistration_view_complete_snapshot() {
        let mut model = UnregisterModel::new();
        UnregisterFeature::update(UnregisterMsg::Unregistered { duration_ms: 456 }, &mut model);
        let mut registry = ActionRegistry::new();
        let components = UnregisterFeature::view(&model, &mut registry);
        let value = serde_json::to_value(&components).unwrap();
        assert_eq!(
            value,
            json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "### Command Unregistration Complete\nSuccessfully unregistered all commands in 456ms"
                        }
                    ]
                }
            ])
        );
    }
}
