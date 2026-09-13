//! The TEA GUI shell for the bot's interactive command features.
//!
//! ## One-way dataflow
//!
//! ```text
//!              ┌─────────────────────────────────────────┐
//!              │         Host (this module, `rt.rs`)       │
//!              │  event loop · collectors · ack · timeout  │
//!              └──────┬──────────────────────▲────────────┘
//!     events/tick     │                      │  Effects (data)
//!                     ▼                      │
//!               ┌─────────┐   update     ┌─────────┐
//!   user input  │ Msg     │─────────────►│ Model   │
//!               └─────────┘  (pure)      └────┬────┘
//!                                            │  & (read-only)
//!                                            ▼
//!                                       ┌─────────┐
//!                                       │  view   │
//!                                       └─────────┘
//! ```
//!
//! ## Layer rules
//!
//! - **Pure core** lives in `crate::update/<feature>`: `Model`, `Msg`,
//!   `Effect`, `update`. No serenity/tokio/DB/diesel.
//! - **Shell** is this module: the [`GuiFeature`](feature::GuiFeature)
//!   contract, the [`Host`](rt::Host) runtime, and the pure `view` / input
//!   translation. The shell may touch serenity types, but never does IO in
//!   `view`.
//! - **Adapters** execute effects through the
//!   [`EffectHandler`](effects::EffectHandler) port — the only IO.
//!
//! Implementors of [`GuiFeature`] are closed to this module.

pub mod about;
pub mod effects;
pub mod feature;
pub mod feed_batch;
pub mod feed_list;
pub mod input;
pub mod register;
pub mod rt;
pub mod unregister;
pub mod voice_leaderboard;
pub mod voice_stats;

pub use effects::EffectHandler;
pub use effects::NoopEffectHandler;
pub use feature::GuiFeature;
pub use rt::Host;

#[cfg(test)]
pub(crate) mod cycle {
    //! Offline TEA-cycle helpers. Each helper drives one seam of the
    //! [`GuiFeature`] contract (view → action by visible label → translate →
    //! update) with constructed boot data: no Discord, no DB.
    //!
    //! [`GuiFeature`]: crate::bot::gui::feature::GuiFeature

    use std::collections::HashSet;

    use crate::bot::gui::feature::GuiFeature;
    use crate::bot::view::Action;
    use crate::bot::view::ActionRegistry;
    use crate::bot::view::SelectValues;

    /// Renders the feature's view into a fresh registry and returns it.
    pub fn view_actions<F: GuiFeature>(model: &F::Model) -> ActionRegistry<F::Action> {
        let mut registry = ActionRegistry::new();
        let _ = F::view(model, &mut registry);
        registry
    }

    /// Finds the action whose [`Action::label()`] value equals `label` —
    /// never by custom id. For variants without an explicit `#[label]` that
    /// value is the variant name, not necessarily the rendered button text;
    /// use [`find_by_rendered_label`] to click what the user actually sees.
    /// Panics when the label is not in the rendered view.
    pub fn find_by_label<T: Action>(registry: &ActionRegistry<T>, label: &str) -> T {
        debug_assert_unique_labels(registry);
        registry
            .actions
            .values()
            .find(|a| a.label() == label)
            .cloned()
            .unwrap_or_else(|| panic!("no action labeled '{label}' in the rendered view"))
    }

    /// Fails debug builds when two actions share a label: [`find_by_label`]
    /// scans a `HashMap`, so a duplicate label would make the picked action
    /// depend on iteration order.
    fn debug_assert_unique_labels<T: Action>(registry: &ActionRegistry<T>) {
        let labels: HashSet<&str> = registry.actions.values().map(|a| a.label()).collect();
        debug_assert_eq!(
            labels.len(),
            registry.actions.len(),
            "duplicate action labels in the rendered view: label lookup would be \
             HashMap-order dependent"
        );
    }

    /// True when an action with this visible label is registered.
    pub fn has_label<T: Action>(registry: &ActionRegistry<T>, label: &str) -> bool {
        registry.actions.values().any(|a| a.label() == label)
    }

    /// Finds the action behind the rendered button the user actually sees:
    /// renders the view, locates the button whose visible text equals
    /// `label`, and resolves its custom id through the registry. Panics when
    /// zero or several rendered buttons carry the label.
    pub fn find_by_rendered_label<F: GuiFeature>(model: &F::Model, label: &str) -> F::Action {
        let mut registry = ActionRegistry::new();
        let components = F::view(model, &mut registry);
        let value = serde_json::to_value(&components).unwrap();
        let ids = rendered_button_ids(&value, label);
        match ids.as_slice() {
            [id] => registry.get(id).cloned().unwrap_or_else(|| {
                panic!("rendered button '{label}' has custom id '{id}', which is not registered")
            }),
            found => {
                panic!("expected one rendered button labeled '{label}', found {found:?}")
            }
        }
    }

    /// Collects the custom ids of rendered buttons whose visible text equals
    /// `label`. A button is an object carrying both `custom_id` and `label`;
    /// select options carry only `label`, so they never match.
    fn rendered_button_ids(value: &serde_json::Value, label: &str) -> Vec<String> {
        match value {
            serde_json::Value::Object(map) => {
                let mut ids = Vec::new();
                ids.extend(button_id(map, label));
                ids.extend(map.values().flat_map(|v| rendered_button_ids(v, label)));
                ids
            }
            serde_json::Value::Array(arr) => arr
                .iter()
                .flat_map(|v| rendered_button_ids(v, label))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The custom id when this JSON object is a button labeled `label`.
    fn button_id(map: &serde_json::Map<String, serde_json::Value>, label: &str) -> Option<String> {
        match (map.get("custom_id"), map.get("label")) {
            (Some(serde_json::Value::String(id)), Some(serde_json::Value::String(text)))
                if text == label =>
            {
                Some(id.clone())
            }
            _ => None,
        }
    }

    /// Renders the feature's view and returns it as normalized snapshot
    /// JSON: registry custom ids become stable `id:Type` sentinels and
    /// Discord `<t:…>` timestamps are redacted, so the shape is reproducible
    /// across runs while still pinning kind/label/style/prefix/order.
    pub fn capture<F: GuiFeature>(model: &F::Model) -> serde_json::Value {
        let mut registry = ActionRegistry::new();
        let components = F::view(model, &mut registry);
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_snapshot(&mut value);
        value
    }

    /// Rewrites registry custom ids (`Type:timestamp:counter`) to
    /// `id:Type` sentinels and redacts `<t:…>` timestamps in rendered JSON.
    fn normalize_snapshot(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::String(cid)) = map.get("custom_id") {
                    let parts: Vec<&str> = cid.split(':').collect();
                    if parts.len() == 3
                        && parts[1].chars().all(|c| c.is_ascii_digit())
                        && parts[2].chars().all(|c| c.is_ascii_digit())
                    {
                        map.insert(
                            "custom_id".to_string(),
                            serde_json::Value::String(format!("id:{}", parts[0])),
                        );
                    }
                }
                for v in map.values_mut() {
                    normalize_snapshot(v);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    normalize_snapshot(v);
                }
            }
            serde_json::Value::String(s) => {
                let redacted = redact_timestamps(s);
                if redacted != *s {
                    *s = redacted;
                }
            }
            _ => {}
        }
    }

    /// Redacts the numeric unix timestamp in a Discord `<t:…>` tag.
    fn redact_timestamps(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(idx) = rest.find("<t:") {
            let (before, after) = rest.split_at(idx);
            out.push_str(before);
            let close = after.find('>').expect("unclosed <t: tag");
            let (tag, remaining) = after.split_at(close + 1);
            let parts: Vec<&str> = tag.split(':').collect();
            out.push_str("<t:TS");
            for p in &parts[2..] {
                out.push(':');
                out.push_str(p);
            }
            rest = remaining;
        }
        out.push_str(rest);
        out
    }

    /// Translates a fired action (button, no select values) into the message
    /// it means. Panics when the action carries no message.
    pub fn translate_action<F: GuiFeature>(action: &F::Action, model: &F::Model) -> F::Msg {
        F::translate(action, SelectValues::String(Vec::new()), model)
            .unwrap_or_else(|| panic!("action {action:?} did not translate to a message"))
    }
}
