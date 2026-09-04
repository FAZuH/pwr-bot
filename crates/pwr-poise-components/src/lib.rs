//! Poise-free builders for Discord message components.
//!
//! Plugins render views as raw Discord message JSON (see `ViewSpec` in
//! `pwr-plugin-protocol`); these builders construct that JSON without pulling
//! in poise or serenity. Every builder returns a [`serde_json::Value`] that
//! matches the Discord wire shape: component `type` numbers, `custom_id`,
//! button styles, select options, and the `flags` field.
//!
//! The full-message builders ([`view_data_v2`] and [`pagination`]) always emit
//! the [`IS_COMPONENTS_V2`] flag and never a top-level `content`: all text
//! rides [`text_display`] components. The legacy content-plus-V2 payload that
//! Discord rejects with error 50035 is unrepresentable through this seam —
//! there is no builder that sets a message-level `content` at all.

use pwr_ext::prelude::CreateButtonDe;
use pwr_ext::prelude::CreateComponentDe;
use pwr_ext::prelude::CreateSelectMenuDe;
use pwr_ext::prelude::CreateSelectMenuOptionDe;
use pwr_ext::view;
use pwr_ext::view_support::ButtonStyle;
use pwr_ext::view_support::CreateActionRow;
use pwr_ext::view_support::CreateComponent;
use pwr_ext::view_support::CreateContainer;
use pwr_ext::view_support::CreateContainerComponent;
use pwr_ext::view_support::CreateSection;
use pwr_ext::view_support::CreateSectionAccessory;
use pwr_ext::view_support::CreateSectionComponent;
use pwr_ext::view_support::CreateSelectMenu;
use pwr_ext::view_support::CreateSelectMenuKind;
use pwr_ext::view_support::CreateSelectMenuOption;
use serde_json::Value;

/// A button component: Discord type 2, style 1 (primary).
pub fn button(custom_id: impl Into<String>, label: impl Into<String>) -> Value {
    let custom_id = custom_id.into();
    let label = label.into();
    first_component(view! {
        action_row { button { custom_id: custom_id, label: label, style: ButtonStyle::Primary } }
    })
}

/// A button with an explicit style. Discord styles are `1..=5`.
///
/// Style 5 preserves this helper's historical payload contract: it includes
/// both the supplied `custom_id` and style `5`. Use [`button_link`] for a
/// Discord link-button payload.
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
    let custom_id = custom_id.into();
    let label = label.into();
    let style = match style {
        1 => ButtonStyle::Primary,
        2 => ButtonStyle::Secondary,
        3 => ButtonStyle::Success,
        4 => ButtonStyle::Danger,
        5 => ButtonStyle::Unknown(5),
        _ => unreachable!(),
    };
    first_component(view! {
        action_row { button { custom_id: custom_id, label: label, style: style } }
    })
}

/// A link-style button: Discord type 2, style 5. Link buttons carry a `url`
/// and no `custom_id`.
pub fn button_link(url: impl Into<String>, label: impl Into<String>) -> Value {
    let url = url.into();
    let label = label.into();
    first_component(view! { action_row { button { url: url, label: label } } })
}

/// An action row: the container every message component must sit in.
/// Discord type 1.
pub fn action_row(children: impl IntoIterator<Item = Value>) -> Value {
    let children = children.into_iter().collect::<Vec<_>>();
    let row = if children
        .first()
        .and_then(|value| value.get("type"))
        .and_then(Value::as_u64)
        == Some(2)
    {
        CreateActionRow::buttons(
            children
                .into_iter()
                .map(|value| serde_json::from_value(value).unwrap())
                .map(|button: CreateButtonDe<'static>| button.into())
                .collect::<Vec<_>>(),
        )
    } else if children.is_empty() {
        CreateActionRow::buttons(Vec::new())
    } else {
        let menu: CreateSelectMenuDe<'static> =
            serde_json::from_value(children.into_iter().next().unwrap()).unwrap();
        CreateActionRow::select_menu(menu)
    };
    let mut output = serde_json::to_value(row).unwrap();
    remove_false_disabled(&mut output);
    output
}

/// A string-select dropdown: Discord type 3.
///
/// # Panics
///
/// Panics if an option is not a valid serialized select-menu option.
pub fn string_select(
    custom_id: impl Into<String>,
    options: impl IntoIterator<Item = Value>,
) -> Value {
    let options = options
        .into_iter()
        .map(|option| {
            let parsed: CreateSelectMenuOptionDe<'static> = serde_json::from_value(option).unwrap();
            CreateSelectMenuOption::from(parsed)
        })
        .collect::<Vec<_>>();
    serde_json::to_value(CreateSelectMenu::new(
        custom_id.into(),
        CreateSelectMenuKind::String {
            options: options.into(),
        },
    ))
    .unwrap()
}

/// A select option: `label` is shown to the user, `value` is what the
/// interaction carries back.
pub fn select_option(label: impl Into<String>, value: impl Into<String>) -> Value {
    serde_json::to_value(CreateSelectMenuOption::new(label.into(), value.into())).unwrap()
}

/// Adds a `description` to an option built by [`select_option`].
///
/// # Panics
///
/// Panics if `option` is not a valid serialized select-menu option.
pub fn select_option_with_description(option: Value, description: impl Into<String>) -> Value {
    let parsed: CreateSelectMenuOptionDe<'static> = serde_json::from_value(option).unwrap();
    serde_json::to_value(CreateSelectMenuOption::from(parsed).description(description.into()))
        .unwrap()
}

/// Marks an option built by [`select_option`] as pre-selected.
///
/// # Panics
///
/// Panics if `option` is not a valid serialized select-menu option.
pub fn with_default(option: Value, default: bool) -> Value {
    let parsed: CreateSelectMenuOptionDe<'static> = serde_json::from_value(option).unwrap();
    serde_json::to_value(CreateSelectMenuOption::from(parsed).default_selection(default)).unwrap()
}

/// The Discord message flag marking a payload as Components V2: every visible
/// element must be a component (text display, section, container, ...), and
/// the legacy top-level `content` must stay empty.
pub const IS_COMPONENTS_V2: u32 = 1 << 15;

/// A text display: Discord type 10. Markdown-only leaf component; the only
/// way to show text in a Components V2 message.
pub fn text_display(content: impl Into<String>) -> Value {
    first_component(view! { components_v2 { text_display { content: content.into() } } })
}

/// A section: Discord type 9. Groups up to three [`text_display`] children
/// with one `accessory` rendered beside them (a thumbnail or a button).
///
/// # Panics
///
/// Panics if a child or accessory is not a supported serialized component.
pub fn section(children: impl IntoIterator<Item = Value>, accessory: Value) -> Value {
    let children: Vec<CreateSectionComponent<'static>> = children
        .into_iter()
        .map(|value| {
            match serde_json::from_value::<CreateComponentDe>(value)
                .unwrap()
                .into()
            {
                CreateComponent::TextDisplay(text) => CreateSectionComponent::TextDisplay(text),
                _ => panic!("section children must be text displays"),
            }
        })
        .collect();
    let accessory = CreateSectionAccessory::Button(
        serde_json::from_value::<pwr_ext::prelude::CreateButtonDe>(accessory)
            .unwrap()
            .into(),
    );
    let mut output = serde_json::to_value(CreateSection::new(children, accessory)).unwrap();
    remove_false_disabled(&mut output);
    output
}

/// A container: Discord type 17. The visual box nesting other components —
/// including other containers — with an optional accent stripe.
///
/// # Panics
///
/// Panics if a child is not a supported serialized container component.
pub fn container(children: impl IntoIterator<Item = Value>) -> Value {
    let children: Vec<CreateContainerComponent<'static>> = children
        .into_iter()
        .map(|value| {
            match serde_json::from_value::<CreateComponentDe>(value)
                .unwrap()
                .into()
            {
                CreateComponent::ActionRow(row) => CreateContainerComponent::ActionRow(row),
                CreateComponent::Section(section) => CreateContainerComponent::Section(section),
                CreateComponent::TextDisplay(text) => CreateContainerComponent::TextDisplay(text),
                CreateComponent::MediaGallery(gallery) => {
                    CreateContainerComponent::MediaGallery(gallery)
                }
                CreateComponent::File(file) => CreateContainerComponent::File(file),
                CreateComponent::Separator(separator) => {
                    CreateContainerComponent::Separator(separator)
                }
                CreateComponent::Container(_) | CreateComponent::Label(_) => {
                    panic!("invalid component inside container")
                }
            }
        })
        .collect();
    let mut output = serde_json::to_value(CreateContainer::new(children)).unwrap();
    remove_false_disabled(&mut output);
    output
}

/// Extracts the public component from a typed message serialization while
/// preserving the legacy helper contract rather than exposing the envelope.
fn first_component(message: impl serde::Serialize) -> Value {
    let mut value = serde_json::to_value(message).unwrap();
    let mut component = value["components"].as_array_mut().unwrap().remove(0);
    if component["type"] == 1 {
        component = component["components"].as_array_mut().unwrap().remove(0);
    }
    remove_false_disabled(&mut component);
    component
}

/// Shapes typed Serenity output back to the legacy visible contract. In
/// particular, it removes default `disabled: false` and empty envelope arrays
/// that Serenity emits but the original helpers did not.
fn remove_false_disabled(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if object.get("disabled") == Some(&Value::Bool(false)) {
                object.remove("disabled");
            }
            for key in ["attachments", "embeds", "sticker_ids"] {
                if object.get(key).is_some_and(Value::is_array)
                    && object[key].as_array().is_some_and(Vec::is_empty)
                {
                    object.remove(key);
                }
            }
            object.values_mut().for_each(remove_false_disabled);
        }
        Value::Array(values) => values.iter_mut().for_each(remove_false_disabled),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Assembles a Components V2 message payload: the given components plus the
/// [`IS_COMPONENTS_V2`] flag. No top-level `content`: all text lives in
/// [`text_display`] components. The explicit `tts` and `enforce_nonce` fields
/// make this full envelope valid for the host's `CreateMessageDe` gate.
///
/// Together with [`pagination`] this is one of the crate's only full-message
/// builders, and neither can express a legacy `content` field.
///
/// # Panics
///
/// Panics if a component cannot be deserialized by the typed pwr-ext wrappers
/// or if Serenity cannot serialize the message.
pub fn view_data_v2(components: impl IntoIterator<Item = Value>) -> Value {
    let components: Vec<pwr_ext::view_support::CreateComponent<'static>> = components
        .into_iter()
        .map(|value| {
            serde_json::from_value::<CreateComponentDe>(value)
                .unwrap()
                .into()
        })
        .collect();
    let mut output = serde_json::to_value(
        pwr_ext::view_support::CreateMessage::new()
            .components(components)
            .flags(serenity_flags(IS_COMPONENTS_V2))
            .tts(false)
            .enforce_nonce(false),
    )
    .unwrap();
    remove_false_disabled(&mut output);
    output
}

fn serenity_flags(value: u32) -> pwr_ext::view_support::MessageFlags {
    pwr_ext::view_support::MessageFlags::from_bits_truncate(value as u16)
}

/// The navigation operations supported by [`pagination`], including its
/// disabled current-page indicator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaginationAction {
    First,
    Prev,
    Next,
    Last,
    Current,
}

impl PaginationAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Prev => "prev",
            Self::Next => "next",
            Self::Last => "last",
            Self::Current => "current",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "first" => Some(Self::First),
            "prev" => Some(Self::Prev),
            "next" => Some(Self::Next),
            "last" => Some(Self::Last),
            "current" => Some(Self::Current),
            _ => None,
        }
    }
}

/// State-independent pagination data. The page is always in `1..=page_count`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Pagination {
    pub page: u32,
    pub page_count: u32,
    pub per_page: u32,
}

impl Pagination {
    /// Creates pagination from an item count. Empty lists still have one page,
    /// and zero items per page is treated as one item per page.
    pub fn new(items: usize, page: u32, per_page: u32) -> Self {
        let per_page = per_page.max(1);
        let item_count = u64::try_from(items).unwrap_or(u64::MAX);
        let page_count = item_count
            .div_ceil(per_page as u64)
            .max(1)
            .try_into()
            .unwrap_or(u32::MAX);
        Self {
            page: page.clamp(1, page_count),
            page_count,
            per_page,
        }
    }

    /// Applies a navigation action without owning or changing caller state.
    pub fn transition(self, action: PaginationAction) -> Self {
        let page = match action {
            PaginationAction::First => 1,
            PaginationAction::Prev => self.page.saturating_sub(1).max(1),
            PaginationAction::Next => self.page.saturating_add(1).min(self.page_count),
            PaginationAction::Last => self.page_count,
            PaginationAction::Current => self.page,
        };
        Self { page, ..self }
    }
}

/// Builds a stable custom ID for a pagination control.
pub fn pagination_custom_id(prefix: &str, action: PaginationAction) -> String {
    assert!(
        !prefix.is_empty(),
        "pagination custom ID prefix must not be empty"
    );
    let id = format!("{prefix}:pagination:{}", action.as_str());
    assert!(
        id.len() <= 100,
        "pagination custom ID must be at most 100 bytes"
    );
    id
}

/// Parses an ID produced by [`pagination_custom_id`]. Prefixes may contain
/// colons; the final pagination marker remains the protocol boundary.
pub fn parse_pagination_custom_id(id: &str) -> Option<(&str, PaginationAction)> {
    if id.len() > 100 {
        return None;
    }
    let (prefix, action) = id.rsplit_once(":pagination:")?;
    if prefix.is_empty() {
        None
    } else {
        Some((prefix, PaginationAction::parse(action)?))
    }
}

/// Builds a Components V2 container containing the page indicator and stable
/// first/previous/next/last controls. The caller owns `Pagination` in its
/// `ViewSpec.view` envelope and uses [`Pagination::transition`] after parsing
/// the clicked ID. This component does not depend on host view traits.
pub fn pagination(
    items: usize,
    page: u32,
    per_page: u32,
    custom_id_prefix: impl AsRef<str>,
) -> Value {
    let state = Pagination::new(items, page, per_page);
    let prefix = custom_id_prefix.as_ref();
    let first_id = pagination_custom_id(prefix, PaginationAction::First);
    let prev_id = pagination_custom_id(prefix, PaginationAction::Prev);
    let next_id = pagination_custom_id(prefix, PaginationAction::Next);
    let last_id = pagination_custom_id(prefix, PaginationAction::Last);
    let current_id = pagination_custom_id(prefix, PaginationAction::Current);
    let first_disabled = state.page == 1;
    let last_disabled = state.page == state.page_count;
    let label = format!("Page {}/{}", state.page, state.page_count);
    let current_label = format!("{}/{}", state.page, state.page_count);

    let message = view! {
        components_v2 {
            container {
                text_display { content: label }
                action_row {
                    button {
                        custom_id: first_id,
                        label: "⏮",
                        style: ButtonStyle::Primary,
                        disabled: first_disabled
                    }
                    button {
                        custom_id: prev_id,
                        label: "◀",
                        style: ButtonStyle::Primary,
                        disabled: first_disabled
                    }
                    button {
                        custom_id: current_id,
                        label: current_label,
                        style: ButtonStyle::Secondary,
                        disabled: true
                    }
                    button {
                        custom_id: next_id,
                        label: "▶",
                        style: ButtonStyle::Primary,
                        disabled: last_disabled
                    }
                    button {
                        custom_id: last_id,
                        label: "⏭",
                        style: ButtonStyle::Primary,
                        disabled: last_disabled
                    }
                }
            }
        }
    };
    serde_json::to_value(message).expect("pagination view is serializable")
}

#[cfg(test)]
mod tests {
    use pwr_ext::prelude::CreateMessageDe;
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
    fn button_with_style_five_preserves_the_legacy_custom_id_contract() {
        assert_eq!(
            serde_json::to_string(&button_with_style("legacy", "Legacy", 5)).unwrap(),
            r#"{"custom_id":"legacy","label":"Legacy","style":5,"type":2}"#
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
    fn message_envelopes_deserialize_through_the_gate() {
        let v2 = view_data_v2([container([text_display("Hello")])]);
        let paged = pagination(12, 2, 5, "feeds");

        serde_json::from_value::<CreateMessageDe>(v2).unwrap();
        serde_json::from_value::<CreateMessageDe>(paged).unwrap();
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

    /// The regression test for Discord error 50035: no full-message builder
    /// may put a legacy `content` field beside the Components V2 flag.
    #[test]
    fn no_full_message_builder_puts_content_beside_the_v2_flag() {
        let built = view_data_v2([container([text_display("Hello")])]);
        assert_eq!(built["flags"], json!(IS_COMPONENTS_V2));
        assert!(
            built.get("content").is_none(),
            "view_data_v2 must not emit legacy content beside the V2 flag"
        );

        let paged = pagination(12, 2, 5, "feeds");
        assert_eq!(paged["flags"], json!(IS_COMPONENTS_V2));
        assert!(
            paged.get("content").is_none(),
            "pagination must not emit legacy content beside the V2 flag"
        );
    }

    #[test]
    fn view_spec_round_trip_preserves_components() {
        let data = view_data_v2([text_display("Hello"), action_row([button("btn", "Click")])]);
        let spec = ViewSpec {
            data,
            ephemeral: true,
            view: json!({"page": 1}),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let back: ViewSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back, spec);
    }

    #[test]
    fn pagination_uses_ceil_pages_and_clamps_inputs() {
        assert_eq!(
            Pagination::new(21, 99, 10),
            Pagination {
                page: 3,
                page_count: 3,
                per_page: 10
            }
        );
        assert_eq!(
            Pagination::new(0, 0, 0),
            Pagination {
                page: 1,
                page_count: 1,
                per_page: 1
            }
        );
        assert_eq!(Pagination::new(20, 0, 0).page_count, 20);
        assert_eq!(Pagination::new(usize::MAX, 0, 1).page_count, u32::MAX);
    }

    #[test]
    fn pagination_transitions_stop_at_both_boundaries() {
        let first = Pagination::new(250, 10, 10);
        assert_eq!(first.transition(PaginationAction::First).page, 1);
        assert_eq!(first.transition(PaginationAction::Prev).page, 9);
        assert_eq!(first.transition(PaginationAction::Next).page, 11);
        assert_eq!(first.transition(PaginationAction::Last).page, 25);
        assert_eq!(
            Pagination::new(250, 1, 10)
                .transition(PaginationAction::Prev)
                .page,
            1
        );
        assert_eq!(
            Pagination::new(250, 25, 10)
                .transition(PaginationAction::Next)
                .page,
            25
        );
    }

    #[test]
    fn pagination_custom_ids_round_trip_and_reject_invalid_ids() {
        let id = pagination_custom_id("settings:feeds", PaginationAction::Next);
        assert_eq!(id, "settings:feeds:pagination:next");
        assert_eq!(
            parse_pagination_custom_id(&id),
            Some(("settings:feeds", PaginationAction::Next))
        );
        assert_eq!(
            parse_pagination_custom_id("settings:feeds:pagination:jump"),
            None
        );
        assert_eq!(parse_pagination_custom_id("pagination:next"), None);
        assert_eq!(
            pagination_custom_id(&"x".repeat(84), PaginationAction::Next).len(),
            100
        );
        assert!(
            std::panic::catch_unwind(|| pagination_custom_id(
                &"x".repeat(85),
                PaginationAction::Next
            ))
            .is_err()
        );
        let current = pagination_custom_id(&"x".repeat(81), PaginationAction::Current);
        assert_eq!(current.len(), 100);
        let (prefix, action) = parse_pagination_custom_id(&current).unwrap();
        assert_eq!(prefix, "x".repeat(81));
        assert_eq!(action, PaginationAction::Current);
        assert_eq!(
            parse_pagination_custom_id(&format!("{}:pagination:next", "x".repeat(85))),
            None
        );
        let utf8_prefix = "é".repeat(40);
        let utf8_id = pagination_custom_id(&utf8_prefix, PaginationAction::Current);
        assert_eq!(utf8_id.len(), 99);
        assert_eq!(parse_pagination_custom_id(&utf8_id).unwrap().0, utf8_prefix);
    }

    #[test]
    #[should_panic(expected = "prefix must not be empty")]
    fn pagination_custom_id_rejects_empty_prefix() {
        pagination_custom_id("", PaginationAction::Next);
    }

    #[test]
    fn pagination_renders_deterministic_message_with_disabled_boundaries() {
        let actual = pagination(21, 3, 10, "settings");
        let expected = json!({
            "attachments": [],
            "components": [{
                "components": [
                    {"content": "Page 3/3", "type": 10},
                    {"components": [
                        {
                            "custom_id": "settings:pagination:first",
                            "disabled": false, "label": "⏮", "style": 1, "type": 2
                        },
                        {
                            "custom_id": "settings:pagination:prev",
                            "disabled": false, "label": "◀", "style": 1, "type": 2
                        },
                        {
                            "custom_id": "settings:pagination:current",
                            "disabled": true, "label": "3/3", "style": 2, "type": 2
                        },
                        {
                            "custom_id": "settings:pagination:next",
                            "disabled": true, "label": "▶", "style": 1, "type": 2
                        },
                        {
                            "custom_id": "settings:pagination:last",
                            "disabled": true, "label": "⏭", "style": 1, "type": 2
                        }
                    ], "type": 1}
                ], "type": 17
            }],
            "flags": 32768,
            "embeds": [],
            "sticker_ids": [],
            "tts": false,
            "enforce_nonce": false
        });
        assert_eq!(actual, expected);
    }

    #[test]
    fn pagination_empty_page_has_valid_single_page_controls() {
        let actual = pagination(0, 1, 0, "empty");
        assert_eq!(actual["flags"], json!(IS_COMPONENTS_V2));
        assert_eq!(
            actual["components"][0]["components"][0]["content"],
            "Page 1/1"
        );
        assert!(
            actual["components"][0]["components"][1]["components"]
                .as_array()
                .unwrap()
                .iter()
                .all(|button| button["disabled"] == json!(true))
        );
    }

    #[test]
    fn pagination_rejects_prefix_that_overflows_current_button_id() {
        assert!(std::panic::catch_unwind(|| pagination(1, 1, 1, "x".repeat(82))).is_err());
    }

    #[test]
    fn pagination_output_is_a_valid_full_message() {
        let data = pagination(12, 2, 5, "feeds");
        let message: CreateMessageDe = serde_json::from_value(data).unwrap();
        let output =
            serde_json::to_value(pwr_ext::view_support::CreateMessage::from(message)).unwrap();
        assert_eq!(output["flags"], json!(IS_COMPONENTS_V2));
        assert_eq!(output["components"][0]["type"], json!(17));
    }
}
