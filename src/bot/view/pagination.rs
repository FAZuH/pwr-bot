//! Pagination row for Discord views.
//!
//! The pagination *intent* and *state* live in the pure core
//! ([`crate::update::pagination`]) and are re-exported here so shells have a
//! single definition of each. This module adds the shell half: the [`Action`]
//! labels and the [`PaginationView`] row renderer, which builds the same five
//! buttons (first/prev/page/next/last) during a feature's `view()` build.

use poise::serenity_prelude::*;

use crate::bot::view::Action;
use crate::bot::view::ActionRegistry;
pub use crate::update::pagination::PaginationAction;
pub use crate::update::pagination::PaginationModel;

impl Action for PaginationAction {
    /// Returns the UI label associated with this action.
    fn label(&self) -> &'static str {
        match self {
            PaginationAction::First => "⏮",
            PaginationAction::Prev => "◀",
            PaginationAction::Page => "Page",
            PaginationAction::Next => "▶",
            PaginationAction::Last => "⏭",
        }
    }
}

#[derive(Clone)]
pub struct PaginationView {
    pub state: PaginationModel,
    pub disabled: bool,
}

impl PaginationView {
    pub fn new(total_items: impl Into<u32>, per_page: impl Into<u32>) -> Self {
        let per_page = per_page.into();
        let pages = total_items.into().div_ceil(per_page);
        let model = PaginationModel::new(pages, per_page, 1);
        Self {
            state: model,
            disabled: false,
        }
    }

    pub fn current_page(&self) -> u32 {
        self.state.current_page
    }

    pub fn attach_if_multipage<'b, T: Action>(
        &self,
        registry: &mut ActionRegistry<T>,
        components: &mut Vec<CreateComponent<'b>>,
        wrap: fn(PaginationAction) -> T,
    ) {
        if !self.disabled && self.state.pages > 1 {
            components.push(self.create_component(registry, wrap));
        }
    }

    pub fn create_component<'b, T: Action>(
        &self,
        registry: &mut ActionRegistry<T>,
        wrap: fn(PaginationAction) -> T,
    ) -> CreateComponent<'b> {
        use pwr_ext::component;

        let first = registry.register(wrap(PaginationAction::First));
        let prev = registry.register(wrap(PaginationAction::Prev));

        let current_page = self.state.current_page;
        let pages = self.state.pages;
        let disabled = self.disabled;

        let next = registry.register(wrap(PaginationAction::Next));
        let last = registry.register(wrap(PaginationAction::Last));

        let row = component! {
            action_row {
                button {
                    custom_id: first.id,
                    label: first.label,
                    style: ButtonStyle::Primary,
                    disabled: current_page == 1 || disabled
                }
                button {
                    custom_id: prev.id,
                    label: prev.label,
                    style: ButtonStyle::Primary,
                    disabled: current_page == 1 || disabled
                }
                button {
                    custom_id: "current",
                    label: format!("{current_page}/{pages}"),
                    style: ButtonStyle::Secondary,
                    disabled: true
                }
                button {
                    custom_id: next.id,
                    label: next.label,
                    style: ButtonStyle::Primary,
                    disabled: current_page == pages || disabled
                }
                button {
                    custom_id: last.id,
                    label: last.label,
                    style: ButtonStyle::Primary,
                    disabled: current_page == pages || disabled
                }
            }
        };

        CreateComponent::ActionRow(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::view::ActionRegistry;

    /// Rewrites every `custom_id` of the shape `Type:timestamp:counter` to a
    /// stable sentinel `id:Type`, so the rendered shape is reproducible across
    /// runs while still pinning kind/label/style/prefix/order.
    fn normalize_custom_ids(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::String(cid)) = map.get("custom_id") {
                    let parts: Vec<&str> = cid.split(':').collect();
                    if parts.len() == 3
                        && parts[1].chars().all(|c| c.is_ascii_digit())
                        && parts[2].chars().all(|c| c.is_ascii_digit())
                    {
                        let replacement = serde_json::json!(format!("id:{}", parts[0]));
                        map.insert("custom_id".to_string(), replacement);
                    }
                }
                for v in map.values_mut() {
                    normalize_custom_ids(v);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    normalize_custom_ids(v);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn pagination_component_middle_page_snapshot() {
        let view = PaginationView {
            state: PaginationModel::new(5, 10, 2),
            disabled: false,
        };
        let mut registry = ActionRegistry::<PaginationAction>::new();
        let component = view.create_component(&mut registry, |a| a);
        let mut value = serde_json::to_value(&component).unwrap();
        normalize_custom_ids(&mut value);
        assert_eq!(
            value,
            serde_json::json!({
                "type": 1,
                "components": [
                    { "type": 2, "custom_id": "id:PaginationAction", "disabled": false, "label": "⏮", "style": 1 },
                    { "type": 2, "custom_id": "id:PaginationAction", "disabled": false, "label": "◀", "style": 1 },
                    { "type": 2, "custom_id": "current", "disabled": true, "label": "2/5", "style": 2 },
                    { "type": 2, "custom_id": "id:PaginationAction", "disabled": false, "label": "▶", "style": 1 },
                    { "type": 2, "custom_id": "id:PaginationAction", "disabled": false, "label": "⏭", "style": 1 }
                ]
            })
        );
    }

    #[test]
    fn pagination_component_first_page_snapshot() {
        let view = PaginationView {
            state: PaginationModel::new(5, 10, 1),
            disabled: false,
        };
        let mut registry = ActionRegistry::<PaginationAction>::new();
        let component = view.create_component(&mut registry, |a| a);
        let mut value = serde_json::to_value(&component).unwrap();
        normalize_custom_ids(&mut value);
        assert_eq!(
            value,
            serde_json::json!({
                "type": 1,
                "components": [
                    { "type": 2, "custom_id": "id:PaginationAction", "disabled": true, "label": "⏮", "style": 1 },
                    { "type": 2, "custom_id": "id:PaginationAction", "disabled": true, "label": "◀", "style": 1 },
                    { "type": 2, "custom_id": "current", "disabled": true, "label": "1/5", "style": 2 },
                    { "type": 2, "custom_id": "id:PaginationAction", "disabled": false, "label": "▶", "style": 1 },
                    { "type": 2, "custom_id": "id:PaginationAction", "disabled": false, "label": "⏭", "style": 1 }
                ]
            })
        );
    }
}
