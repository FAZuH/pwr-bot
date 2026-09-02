//! The `/register` feature shell — a [`GuiFeature`] over the pure register
//! core.
//!
//! A one-shot command view: the handler drives it directly (creating the model
//! from the command count, sending the registering reply, then applying the
//! [`RegisterMsg::Registered`] result and editing to the complete reply).

use pwr_ext::component;

use crate::bot::command::prelude::*;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::feature::sealed;
use crate::bot::view::SelectValues;
use crate::update::register::RegisterEffect;
use crate::update::register::RegisterModel;
use crate::update::register::RegisterMsg;
use crate::update::register::update as register_update;

/// Data-in for the registration feature: the number of commands to register.
pub struct RegisterConfig {
    pub num_commands: usize,
}

/// The registration view has no interactive actions (it is a one-shot status
/// view, never rendered with a component registry in a live loop).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterAction {}

impl crate::bot::view::Action for RegisterAction {
    fn label(&self) -> &'static str {
        match *self {}
    }
}

/// The registration feature.
pub struct RegisterFeature;

impl sealed::Sealed for RegisterFeature {}

impl GuiFeature for RegisterFeature {
    type Model = RegisterModel;
    type Msg = RegisterMsg;
    type Action = RegisterAction;
    type Effect = RegisterEffect;
    type Config = RegisterConfig;

    fn initial(config: Self::Config) -> Self::Model {
        RegisterModel::new(config.num_commands)
    }

    fn start_msg() -> Self::Msg {
        RegisterMsg::Start
    }

    fn timeout_msg() -> Self::Msg {
        RegisterMsg::Expired
    }

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Vec<Self::Effect> {
        register_update(msg, model)
    }

    fn view<'a>(
        model: &'a Self::Model,
        _registry: &mut ActionRegistry<Self::Action>,
    ) -> Vec<CreateComponent<'a>> {
        let title = if model.is_complete {
            "Command Registration Complete"
        } else {
            "Registering Commands"
        };

        let status_text = if model.is_complete {
            format!(
                "### {}\nSuccessfully registered {} commands in {}ms",
                title,
                model.num_commands,
                model.duration_ms.unwrap_or(0)
            )
        } else {
            format!(
                "### {}\nRegistering {} server commands...",
                title, model.num_commands
            )
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
    fn registration_view_incomplete_snapshot() {
        let model = RegisterModel::new(5);
        let mut registry = ActionRegistry::new();
        let components = RegisterFeature::view(&model, &mut registry);
        let value = serde_json::to_value(&components).unwrap();
        assert_eq!(
            value,
            json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "### Registering Commands\nRegistering 5 server commands..."
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn registration_view_complete_snapshot() {
        let mut model = RegisterModel::new(5);
        RegisterFeature::update(RegisterMsg::Registered { duration_ms: 1234 }, &mut model);
        let mut registry = ActionRegistry::new();
        let components = RegisterFeature::view(&model, &mut registry);
        let value = serde_json::to_value(&components).unwrap();
        assert_eq!(
            value,
            json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "### Command Registration Complete\nSuccessfully registered 5 commands in 1234ms"
                        }
                    ]
                }
            ])
        );
    }
}
