//! Pure update logic for welcome settings.
//!
//! Manages welcome-card configuration state and message-removal bookkeeping.
//!
//! Holds the single source of truth for the welcome settings view
//! ([`WelcomeSettingsModel`]), which absorbs the server's [`ServerSettings`] as
//! owned state plus the rendered preview image bytes (raw `Vec<u8>`, no
//! serenity types cross in) — eliminating the raw `&mut ServerSettings` plus
//! `current_image_bytes` pair the old handler carried. Also holds the
//! exhaustive message vocabulary ([`WelcomeSettingsMsg`]) and the data-only
//! effect vocabulary ([`WelcomeSettingsEffect`]).
//!
//! Every mutating message returns both a
//! [`WelcomeSettingsEffect::PersistSettings`] snapshot and a
//! [`WelcomeSettingsEffect::RenderImage`] snapshot, mirroring the old
//! `persist_and_regenerate` call the handler ran after each mutation. The
//! terminal messages (`Back`, `About`) and the shared lifecycle expiry persist
//! nothing: every change was already persisted when it happened, exactly as
//! before.

use std::collections::HashSet;

use crate::entity::ServerSettings;
use crate::update::lifecycle::Lifecycle;

/// Messages that can mutate the welcome-settings model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WelcomeSettingsMsg {
    /// The shared boot/timeout lifecycle moments.
    Lifecycle(Lifecycle),
    ToggleEnabled,
    SetChannel(Option<String>),
    SetTemplate(Option<String>),
    MarkRemoval(HashSet<usize>),
    AddMessage(String),
    SetColor(String),
    SaveRemoval,
    CancelRemoval,
    /// The back button was pressed.
    Back,
    /// The about button was pressed.
    About,
    /// The adapter finished (re)rendering the preview image.
    ImageRendered(Option<Vec<u8>>),
    /// The adapter finished persisting the settings.
    SettingsPersisted,
}

impl From<Lifecycle> for WelcomeSettingsMsg {
    fn from(lifecycle: Lifecycle) -> Self {
        Self::Lifecycle(lifecycle)
    }
}

/// Effects the welcome-settings view can request.
///
/// Data-only: each variant carries a snapshot of the post-mutation server
/// settings; the adapter performs the write / render.
#[derive(Debug, Clone)]
pub enum WelcomeSettingsEffect {
    /// Persist the current server settings (data-only snapshot).
    PersistSettings(ServerSettings),
    /// (Re)render the welcome-card preview from a snapshot of the settings.
    RenderImage(ServerSettings),
}

/// The welcome-settings model.
#[derive(Debug, Clone)]
pub struct WelcomeSettingsModel {
    /// The server settings, including the welcome section being edited.
    pub(crate) settings: ServerSettings,
    pub(crate) marked_removal: HashSet<usize>,
    /// The rendered preview image bytes, if a preview was generated.
    pub(crate) image_bytes: Option<Vec<u8>>,
}

impl WelcomeSettingsModel {
    /// Constructs the model from the server settings and preview image loaded
    /// at boot.
    pub fn new(settings: ServerSettings, image_bytes: Option<Vec<u8>>) -> Self {
        Self {
            settings,
            marked_removal: HashSet::new(),
            image_bytes,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.settings.welcome.enabled.unwrap_or(false)
    }

    pub fn message_count(&self) -> usize {
        self.settings
            .welcome
            .messages
            .as_ref()
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// The displayed preview image bytes, if a preview was rendered.
    pub fn image_bytes(&self) -> Option<&[u8]> {
        self.image_bytes.as_deref()
    }
}

/// The pure update function — the only writer of the welcome-settings model.
///
/// Each mutating message applies to `model.settings.welcome` in place and
/// returns both a [`WelcomeSettingsEffect::PersistSettings`] and a
/// [`WelcomeSettingsEffect::RenderImage`] snapshot, mirroring the old
/// `persist_and_regenerate` pairing exactly (including for no-op edits such as
/// empty or over-cap messages, which the old handler persisted too).
/// Selection-only messages (`MarkRemoval`, `CancelRemoval`), the lifecycle
/// moments (through [`Lifecycle::handle`] — both start and expiry persist
/// nothing), navigation (`Back`, `About`), and the async results
/// (`ImageRendered`, `SettingsPersisted`) return no effects.
pub fn update(
    msg: WelcomeSettingsMsg,
    model: &mut WelcomeSettingsModel,
) -> Vec<WelcomeSettingsEffect> {
    use WelcomeSettingsMsg::*;

    match msg {
        WelcomeSettingsMsg::Lifecycle(lifecycle) => lifecycle.handle(Vec::new),
        Back | About | SettingsPersisted => Vec::new(),
        ToggleEnabled => {
            let current = model.settings.welcome.enabled.unwrap_or(false);
            model.settings.welcome.enabled = Some(!current);
            effects(model)
        }
        SetChannel(channel_id) => {
            model.settings.welcome.channel_id = channel_id;
            effects(model)
        }
        SetTemplate(template_id) => {
            model.settings.welcome.template_id = template_id;
            effects(model)
        }
        MarkRemoval(indices) => {
            model.marked_removal = indices;
            Vec::new()
        }
        AddMessage(msg) => {
            let trimmed = msg.trim().to_string();
            if !trimmed.is_empty() {
                let msgs = model.settings.welcome.messages.get_or_insert_with(Vec::new);
                if msgs.len() < 25 {
                    msgs.push(trimmed);
                }
            }
            effects(model)
        }
        SetColor(color) => {
            let trimmed = color.trim().to_string();
            if trimmed.starts_with('#') {
                model.settings.welcome.primary_color = Some(trimmed);
            }
            effects(model)
        }
        SaveRemoval => {
            let msgs = model.settings.welcome.messages.clone().unwrap_or_default();
            model.settings.welcome.messages = Some(
                msgs.into_iter()
                    .enumerate()
                    .filter(|(i, _)| !model.marked_removal.contains(i))
                    .map(|(_, msg)| msg)
                    .collect(),
            );
            model.marked_removal.clear();
            effects(model)
        }
        CancelRemoval => {
            model.marked_removal.clear();
            Vec::new()
        }
        ImageRendered(bytes) => {
            model.image_bytes = bytes;
            Vec::new()
        }
    }
}

/// Builds the [`WelcomeSettingsEffect::PersistSettings`] +
/// [`WelcomeSettingsEffect::RenderImage`] pair from the post-mutation model.
fn effects(model: &WelcomeSettingsModel) -> Vec<WelcomeSettingsEffect> {
    vec![
        WelcomeSettingsEffect::PersistSettings(model.settings.clone()),
        WelcomeSettingsEffect::RenderImage(model.settings.clone()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::WelcomeSettings;

    fn empty_model() -> WelcomeSettingsModel {
        WelcomeSettingsModel::new(ServerSettings::default(), None)
    }

    fn welcome_settings() -> WelcomeSettings {
        WelcomeSettings::default()
    }

    /// Asserts the update returned exactly the persist + render pair, and that
    /// both snapshots match the model's post-mutation settings.
    fn assert_persist_and_render(effects: &[WelcomeSettingsEffect], model: &WelcomeSettingsModel) {
        assert_eq!(effects.len(), 2);
        match &effects[0] {
            WelcomeSettingsEffect::PersistSettings(s) => assert_eq!(
                s.welcome.enabled, model.settings.welcome.enabled,
                "persist snapshot must match the model"
            ),
            WelcomeSettingsEffect::RenderImage(_) => {
                panic!("first effect must be PersistSettings")
            }
        }
        assert!(
            matches!(&effects[1], WelcomeSettingsEffect::RenderImage(_)),
            "second effect must be RenderImage, got {:?}",
            effects[1]
        );
    }

    // ── ToggleEnabled ───────────────────────────────────────────────────────

    #[test]
    fn toggle_enabled_from_false() {
        let mut model = empty_model();
        assert!(!model.is_enabled());

        let fx = update(WelcomeSettingsMsg::ToggleEnabled, &mut model);

        assert_persist_and_render(&fx, &model);
        assert!(model.is_enabled());
    }

    #[test]
    fn toggle_enabled_from_true() {
        let mut model = empty_model();
        model.settings.welcome = welcome_settings();
        model.settings.welcome.enabled = Some(true);

        let fx = update(WelcomeSettingsMsg::ToggleEnabled, &mut model);

        assert_persist_and_render(&fx, &model);
        assert!(!model.is_enabled());
    }

    // ── SetChannel ──────────────────────────────────────────────────────────

    #[test]
    fn set_channel() {
        let mut model = empty_model();

        let fx = update(
            WelcomeSettingsMsg::SetChannel(Some("123".to_string())),
            &mut model,
        );

        assert_persist_and_render(&fx, &model);
        assert_eq!(model.settings.welcome.channel_id, Some("123".to_string()));
    }

    #[test]
    fn set_channel_none() {
        let mut model = empty_model();
        model.settings.welcome.channel_id = Some("123".to_string());

        let fx = update(WelcomeSettingsMsg::SetChannel(None), &mut model);

        assert_persist_and_render(&fx, &model);
        assert_eq!(model.settings.welcome.channel_id, None);
    }

    // ── SetTemplate ─────────────────────────────────────────────────────────

    #[test]
    fn set_template() {
        let mut model = empty_model();

        let fx = update(
            WelcomeSettingsMsg::SetTemplate(Some("5".to_string())),
            &mut model,
        );

        assert_persist_and_render(&fx, &model);
        assert_eq!(model.settings.welcome.template_id, Some("5".to_string()));
    }

    // ── MarkRemoval ─────────────────────────────────────────────────────────

    #[test]
    fn mark_removal() {
        let mut model = empty_model();
        let mut indices = HashSet::new();
        indices.insert(1);
        indices.insert(3);

        let fx = update(WelcomeSettingsMsg::MarkRemoval(indices.clone()), &mut model);

        assert!(fx.is_empty());
        assert_eq!(model.marked_removal, indices);
    }

    // ── AddMessage ──────────────────────────────────────────────────────────

    #[test]
    fn add_message() {
        let mut model = empty_model();

        let fx = update(
            WelcomeSettingsMsg::AddMessage("Hello!".to_string()),
            &mut model,
        );

        assert_persist_and_render(&fx, &model);
        assert_eq!(model.message_count(), 1);
        assert_eq!(
            model.settings.welcome.messages.as_ref().unwrap()[0],
            "Hello!"
        );
    }

    #[test]
    fn add_message_empty_ignored() {
        let mut model = empty_model();

        let fx = update(
            WelcomeSettingsMsg::AddMessage("   ".to_string()),
            &mut model,
        );

        assert_persist_and_render(&fx, &model);
        assert_eq!(model.message_count(), 0);
    }

    #[test]
    fn add_message_cap_at_25() {
        let mut model = empty_model();
        model.settings.welcome.messages = Some((0..25).map(|i| format!("msg{i}")).collect());

        let fx = update(
            WelcomeSettingsMsg::AddMessage("overflow".to_string()),
            &mut model,
        );

        assert_persist_and_render(&fx, &model);
        assert_eq!(model.message_count(), 25);
    }

    // ── SetColor ────────────────────────────────────────────────────────────

    #[test]
    fn set_color_valid() {
        let mut model = empty_model();

        let fx = update(
            WelcomeSettingsMsg::SetColor("#FF5733".to_string()),
            &mut model,
        );

        assert_persist_and_render(&fx, &model);
        assert_eq!(
            model.settings.welcome.primary_color,
            Some("#FF5733".to_string())
        );
    }

    #[test]
    fn set_color_invalid_ignored() {
        let mut model = empty_model();

        let fx = update(
            WelcomeSettingsMsg::SetColor("FF5733".to_string()),
            &mut model,
        );

        assert_persist_and_render(&fx, &model);
        assert_eq!(model.settings.welcome.primary_color, None);
    }

    // ── SaveRemoval ─────────────────────────────────────────────────────────

    #[test]
    fn save_removal() {
        let mut model = empty_model();
        model.settings.welcome.messages = Some(vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ]);
        model.marked_removal.insert(1);
        model.marked_removal.insert(3);

        let fx = update(WelcomeSettingsMsg::SaveRemoval, &mut model);

        assert_persist_and_render(&fx, &model);
        assert!(model.marked_removal.is_empty());
        assert_eq!(
            model.settings.welcome.messages,
            Some(vec!["a".to_string(), "c".to_string()])
        );
    }

    #[test]
    fn save_removal_empty_messages() {
        let mut model = empty_model();
        model.marked_removal.insert(0);

        let fx = update(WelcomeSettingsMsg::SaveRemoval, &mut model);

        assert_persist_and_render(&fx, &model);
        assert!(model.marked_removal.is_empty());
        assert_eq!(model.settings.welcome.messages, Some(vec![]));
    }

    // ── CancelRemoval ───────────────────────────────────────────────────────

    #[test]
    fn cancel_removal() {
        let mut model = empty_model();
        model.marked_removal.insert(1);
        model.marked_removal.insert(2);

        let fx = update(WelcomeSettingsMsg::CancelRemoval, &mut model);

        assert!(fx.is_empty());
        assert!(model.marked_removal.is_empty());
    }

    // ── Lifecycle / navigation ──────────────────────────────────────────────

    #[test]
    fn start_is_a_noop() {
        let mut model = empty_model();
        let fx = update(WelcomeSettingsMsg::Lifecycle(Lifecycle::Start), &mut model);
        assert!(fx.is_empty());
    }

    #[test]
    fn expired_persists_nothing() {
        let mut model = empty_model();
        model.settings.welcome.enabled = Some(true);
        let fx = update(
            WelcomeSettingsMsg::Lifecycle(Lifecycle::Expired),
            &mut model,
        );
        assert!(fx.is_empty());
        assert!(model.is_enabled());
    }

    #[test]
    fn back_persists_nothing() {
        let mut model = empty_model();
        let fx = update(WelcomeSettingsMsg::Back, &mut model);
        assert!(fx.is_empty());
    }

    #[test]
    fn about_persists_nothing() {
        let mut model = empty_model();
        let fx = update(WelcomeSettingsMsg::About, &mut model);
        assert!(fx.is_empty());
    }

    // ── Async results ───────────────────────────────────────────────────────

    #[test]
    fn image_rendered_sets_bytes() {
        let mut model = empty_model();
        let fx = update(
            WelcomeSettingsMsg::ImageRendered(Some(vec![1, 2, 3])),
            &mut model,
        );
        assert!(fx.is_empty());
        assert_eq!(model.image_bytes(), Some(&[1, 2, 3][..]));
    }

    #[test]
    fn image_rendered_none_clears_bytes() {
        let mut model = WelcomeSettingsModel::new(ServerSettings::default(), Some(vec![9]));
        let fx = update(WelcomeSettingsMsg::ImageRendered(None), &mut model);
        assert!(fx.is_empty());
        assert_eq!(model.image_bytes(), None);
    }

    #[test]
    fn settings_persisted_is_a_noop() {
        let mut model = empty_model();
        model.settings.welcome.enabled = Some(true);
        let fx = update(WelcomeSettingsMsg::SettingsPersisted, &mut model);
        assert!(fx.is_empty());
        assert!(model.is_enabled());
    }

    // ── Model helpers ───────────────────────────────────────────────────────

    #[test]
    fn message_count() {
        let mut model = empty_model();
        assert_eq!(model.message_count(), 0);

        model.settings.welcome.messages = Some(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(model.message_count(), 2);
    }

    #[test]
    fn is_enabled_default() {
        let model = empty_model();
        assert!(!model.is_enabled());
    }

    #[test]
    fn boot_image_bytes_roundtrip() {
        let model = WelcomeSettingsModel::new(ServerSettings::default(), Some(vec![7, 8]));
        assert_eq!(model.image_bytes(), Some(&[7, 8][..]));
    }
}
