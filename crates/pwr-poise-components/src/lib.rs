//! Poise-free builders for Discord message components.
//!
//! Plugins render views as raw Discord message JSON (see `ViewSpec` in
//! `pwr-plugin-protocol`); these builders construct that JSON without pulling
//! in poise or serenity. Every builder returns a [`serde_json::Value`] that
//! matches the Discord wire shape: component `type` numbers, `custom_id`,
//! button styles, select options, and the `flags` field.

use serde_json::Value;
use serde_json::json;

/// A button component: Discord type 2, style 1 (primary).
pub fn button(custom_id: impl Into<String>, label: impl Into<String>) -> Value {
    json!({
        "type": 2,
        "custom_id": custom_id.into(),
        "label": label.into(),
        "style": 1
    })
}

/// A button with an explicit style. Discord styles are `1..=5`; style 5
/// (link) carries a `url` and no `custom_id` — use [`button_link`] for that.
///
/// # Panics
///
/// Panics if `style` is outside `1..=5`.
pub fn button_with_style(
    custom_id: impl Into<String>,
    label: impl Into<String>,
    style: u8,
) -> Value {
    assert!(
        (1..=5).contains(&style),
        "button style must be 1..=5, got {style}"
    );
    json!({
        "type": 2,
        "custom_id": custom_id.into(),
        "label": label.into(),
        "style": style
    })
}

/// A link-style button: Discord type 2, style 5. Link buttons carry a `url`
/// and no `custom_id`.
pub fn button_link(url: impl Into<String>, label: impl Into<String>) -> Value {
    json!({
        "type": 2,
        "style": 5,
        "label": label.into(),
        "url": url.into()
    })
}

/// An action row: the container every message component must sit in.
/// Discord type 1.
pub fn action_row(children: impl IntoIterator<Item = Value>) -> Value {
    json!({
        "type": 1,
        "components": children.into_iter().collect::<Vec<_>>()
    })
}

/// A string-select dropdown: Discord type 3.
pub fn string_select(
    custom_id: impl Into<String>,
    options: impl IntoIterator<Item = Value>,
) -> Value {
    json!({
        "type": 3,
        "custom_id": custom_id.into(),
        "options": options.into_iter().collect::<Vec<_>>()
    })
}

/// A select option: `label` is shown to the user, `value` is what the
/// interaction carries back.
pub fn select_option(label: impl Into<String>, value: impl Into<String>) -> Value {
    json!({
        "label": label.into(),
        "value": value.into()
    })
}

/// Adds a `description` to an option built by [`select_option`]. No-op if the
/// value is not a JSON object.
pub fn select_option_with_description(mut option: Value, description: impl Into<String>) -> Value {
    if let Some(object) = option.as_object_mut() {
        object.insert("description".into(), json!(description.into()));
    }
    option
}

/// Marks an option built by [`select_option`] as pre-selected. No-op if the
/// value is not a JSON object.
pub fn with_default(mut option: Value, default: bool) -> Value {
    if let Some(object) = option.as_object_mut() {
        object.insert("default".into(), json!(default));
    }
    option
}

/// Assembles a full message payload: content plus the given components.
/// `flags: 0` keeps the message free of special Discord flags.
pub fn view_data(content: impl Into<String>, components: impl IntoIterator<Item = Value>) -> Value {
    json!({
        "content": content.into(),
        "components": components.into_iter().collect::<Vec<_>>(),
        "flags": 0
    })
}

/// The Discord message flag marking a payload as Components V2: every visible
/// element must be a component (text display, section, container, ...), and
/// the legacy top-level `content` must stay empty.
pub const IS_COMPONENTS_V2: u32 = 1 << 15;

/// A text display: Discord type 10. Markdown-only leaf component; the only
/// way to show text in a Components V2 message.
pub fn text_display(content: impl Into<String>) -> Value {
    json!({
        "type": 10,
        "content": content.into()
    })
}

/// A section: Discord type 9. Groups up to three [`text_display`] children
/// with one `accessory` rendered beside them (a thumbnail or a button).
pub fn section(children: impl IntoIterator<Item = Value>, accessory: Value) -> Value {
    json!({
        "type": 9,
        "components": children.into_iter().collect::<Vec<_>>(),
        "accessory": accessory
    })
}

/// A container: Discord type 17. The visual box nesting other components —
/// including other containers — with an optional accent stripe.
pub fn container(children: impl IntoIterator<Item = Value>) -> Value {
    json!({
        "type": 17,
        "components": children.into_iter().collect::<Vec<_>>()
    })
}

/// Assembles a Components V2 message payload: the given components plus the
/// [`IS_COMPONENTS_V2`] flag. No top-level `content`: all text lives in
/// [`text_display`] components.
pub fn view_data_v2(components: impl IntoIterator<Item = Value>) -> Value {
    json!({
        "components": components.into_iter().collect::<Vec<_>>(),
        "flags": IS_COMPONENTS_V2
    })
}

#[cfg(test)]
mod tests {
    use pwr_plugin_protocol::ViewSpec;
    use serde_json::json;

    use super::*;

    #[test]
    fn button_emits_the_discord_shape() {
        assert_eq!(
            serde_json::to_string(&button("btn", "Click")).unwrap(),
            r#"{"custom_id":"btn","label":"Click","style":1,"type":2}"#
        );
    }

    #[test]
    fn button_with_style_carries_the_given_style() {
        assert_eq!(
            serde_json::to_string(&button_with_style("btn", "Click", 4)).unwrap(),
            r#"{"custom_id":"btn","label":"Click","style":4,"type":2}"#
        );
    }

    #[test]
    fn button_link_requires_url_and_omits_custom_id() {
        let link = button_link("https://example.com", "Docs");
        assert_eq!(
            serde_json::to_string(&link).unwrap(),
            r#"{"label":"Docs","style":5,"type":2,"url":"https://example.com"}"#
        );
        assert!(link.get("custom_id").is_none());
    }

    #[test]
    fn action_row_wraps_its_children() {
        let child = button("btn", "Click");
        let row = action_row([child.clone()]);
        assert_eq!(row["type"], 1);
        assert_eq!(row["components"][0], child);
    }

    #[test]
    fn action_row_with_no_children() {
        assert_eq!(
            serde_json::to_string(&action_row(Vec::<Value>::new())).unwrap(),
            r#"{"components":[],"type":1}"#
        );
    }

    #[test]
    fn string_select_emits_the_discord_shape() {
        let option = select_option("A", "a");
        let select = string_select("sel", [option.clone()]);
        assert_eq!(select["type"], 3);
        assert_eq!(select["custom_id"], "sel");
        assert_eq!(select["options"][0], option);
    }

    #[test]
    fn select_option_carries_label_and_value() {
        assert_eq!(
            serde_json::to_string(&select_option("A", "a")).unwrap(),
            r#"{"label":"A","value":"a"}"#
        );
    }

    #[test]
    fn select_option_with_description_adds_description() {
        let option = select_option_with_description(select_option("A", "a"), "first");
        assert_eq!(
            serde_json::to_string(&option).unwrap(),
            r#"{"description":"first","label":"A","value":"a"}"#
        );
    }

    #[test]
    fn with_default_marks_the_option() {
        let option = with_default(select_option("A", "a"), true);
        assert_eq!(
            serde_json::to_string(&option).unwrap(),
            r#"{"default":true,"label":"A","value":"a"}"#
        );
    }

    #[test]
    fn view_data_assembles_content_and_components() {
        let data = view_data("Hello", [action_row([button("btn", "Click")])]);
        assert_eq!(
            data,
            json!({
                "content": "Hello",
                "components": [{
                    "type": 1,
                    "components": [{
                        "type": 2,
                        "custom_id": "btn",
                        "label": "Click",
                        "style": 1
                    }]
                }],
                "flags": 0
            })
        );
    }

    #[test]
    fn text_display_emits_the_discord_shape() {
        assert_eq!(
            serde_json::to_string(&text_display("-# **Settings**")).unwrap(),
            r#"{"content":"-# **Settings**","type":10}"#
        );
    }

    #[test]
    fn section_wraps_text_displays_and_the_accessory() {
        let text = text_display("hello");
        let accessory = button("btn", "Click");
        let section = section([text.clone()], accessory.clone());
        assert_eq!(section["type"], 9);
        assert_eq!(section["components"][0], text);
        assert_eq!(section["accessory"], accessory);
    }

    #[test]
    fn container_wraps_its_children() {
        let child = text_display("hello");
        let boxed = container([child.clone()]);
        assert_eq!(boxed["type"], 17);
        assert_eq!(boxed["components"][0], child);
    }

    #[test]
    fn view_data_v2_sets_the_components_v2_flag_and_omits_content() {
        let data = view_data_v2([container([text_display("-# **Settings**")])]);
        assert_eq!(data["flags"], json!(IS_COMPONENTS_V2));
        assert!(data.get("content").is_none());
        assert_eq!(data["components"][0]["type"], 17);
    }

    #[test]
    fn view_spec_round_trip_preserves_components() {
        let data = view_data("Hello", [action_row([button("btn", "Click")])]);
        let spec = ViewSpec {
            data,
            ephemeral: true,
            view: json!({"page": 1}),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let back: ViewSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back, spec);
    }
}
