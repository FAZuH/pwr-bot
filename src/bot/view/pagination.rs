//! Pagination component for Discord views.
use poise::serenity_prelude::*;
use pwr_ext::component;

use crate::action_enum;
use crate::bot::Error;
use crate::bot::view::Action;
use crate::bot::view::ActionRegistry;
use crate::bot::view::ViewCmd;
use crate::bot::view::ViewContext;
use crate::bot::view::ViewHandler;

/// Model for tracking pagination state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaginationModel {
    pub current_page: u32,
    pub pages: u32,
    #[allow(dead_code)]
    pub per_page: u32,
}

impl PaginationModel {
    /// Creates a new pagination model with the given parameters.
    pub fn new(pages: u32, per_page: u32, current_page: u32) -> Self {
        let pages = pages.max(1);
        let per_page = per_page.max(1);
        let current_page = current_page.clamp(1, pages.max(1));
        Self {
            pages,
            per_page,
            current_page,
        }
    }

    /// Navigates to the first page.
    pub fn first_page(&mut self) {
        self.current_page = 1;
    }

    /// Navigates to the previous page if not on the first page.
    pub fn prev_page(&mut self) {
        if self.current_page > 1 {
            self.current_page -= 1;
        }
    }

    /// Navigates to the next page if not on the last page.
    pub fn next_page(&mut self) {
        if self.current_page < self.pages {
            self.current_page += 1;
        }
    }

    /// Navigates to the last page.
    pub fn last_page(&mut self) {
        self.current_page = self.pages;
    }
}

action_enum!(
    #[derive(Copy)]
    PaginationAction {
        #[label = "⏮"]
        First,
        #[label = "◀"]
        Prev,
        Page,
        #[label = "▶"]
        Next,
        #[label = "⏭"]
        Last,
    }
);

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

#[async_trait::async_trait]
impl ViewHandler for PaginationView {
    type Action = PaginationAction;
    async fn handle(&mut self, ctx: ViewContext<'_, PaginationAction>) -> Result<ViewCmd, Error> {
        match ctx.action() {
            PaginationAction::First => self.state.first_page(),
            PaginationAction::Prev => self.state.prev_page(),
            PaginationAction::Next => self.state.next_page(),
            PaginationAction::Last => self.state.last_page(),
            _ => return Ok(ViewCmd::Continue),
        }
        Ok(ViewCmd::Render)
    }

    async fn on_timeout(&mut self) -> Result<ViewCmd, Error> {
        self.disabled = true;
        if self.state.pages > 1 {
            Ok(ViewCmd::RenderOnce)
        } else {
            Ok(ViewCmd::Exit)
        }
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

    #[test]
    fn pagination_new() {
        // Normal case
        let p = PaginationModel::new(10, 5, 1);
        assert_eq!(p.pages, 10);
        assert_eq!(p.per_page, 5);
        assert_eq!(p.current_page, 1);

        // Clamping current_page
        let p = PaginationModel::new(10, 5, 0);
        assert_eq!(p.current_page, 1);

        let p = PaginationModel::new(10, 5, 11);
        assert_eq!(p.current_page, 10);

        // Minimal values
        let p = PaginationModel::new(0, 0, 0);
        assert_eq!(p.pages, 1);
        assert_eq!(p.per_page, 1);
        assert_eq!(p.current_page, 1);
    }

    #[test]
    fn pagination_navigation() {
        let mut p = PaginationModel::new(5, 10, 3);

        p.prev_page();
        assert_eq!(p.current_page, 2);

        p.prev_page();
        assert_eq!(p.current_page, 1);

        p.prev_page();
        assert_eq!(p.current_page, 1); // Should not go below 1

        p.next_page();
        assert_eq!(p.current_page, 2);

        p.last_page();
        assert_eq!(p.current_page, 5);

        p.next_page();
        assert_eq!(p.current_page, 5); // Should not go above pages

        p.first_page();
        assert_eq!(p.current_page, 1);
    }
}
