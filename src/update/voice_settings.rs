//! Pure update logic for the `/vc settings` command.
//!
//! Holds the single source of truth for the voice settings view
//! (`VoiceSettingsModel`), which absorbs the server's [`ServerSettings`] as
//! owned state — eliminating the raw `&mut ServerSettings` that the old
//! handler carried. Also holds the exhaustive message vocabulary
//! (`VoiceSettingsMsg`) and the data-only effect vocabulary
//! (`VoiceSettingsEffect`).
//!
//! Persistence is now an explicit effect, not an implicit save-on-exit: the
//! terminal messages (`Back`, `About`, `Expired`) return a
//! [`VoiceSettingsEffect::PersistSettings`] snapshot that the shell adapter
//! executes when the host loop ends.

use crate::entity::ServerSettings;

/// The voice settings view model — the single source of truth for the view.
#[derive(Debug, Clone)]
pub struct VoiceSettingsModel {
    /// The server settings, including the voice section being edited.
    pub(crate) settings: ServerSettings,
}

impl VoiceSettingsModel {
    /// Constructs the model from the server settings loaded at boot.
    pub fn new(settings: ServerSettings) -> Self {
        Self { settings }
    }

    /// Whether voice tracking is currently enabled (defaults to enabled).
    pub fn voice_enabled(&self) -> bool {
        self.settings.voice.enabled.unwrap_or(true)
    }
}

/// Messages that drive the voice settings view.
///
/// Exhaustive: every way the world can change the voice settings model is one
/// variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceSettingsMsg {
    /// Boot handshake — the host dispatches this on start.
    Start,
    /// The view loop timed out.
    Expired,
    /// Toggle whether voice tracking is enabled.
    ToggleEnabled,
    /// The back button was pressed.
    Back,
    /// The about button was pressed.
    About,
}

/// Effects the voice settings view can request.
///
/// Data-only: [`VoiceSettingsEffect::PersistSettings`] carries a snapshot of
/// the settings to persist; the adapter performs the write.
#[derive(Debug, Clone)]
pub enum VoiceSettingsEffect {
    /// Persist the current server settings (data-only snapshot).
    PersistSettings(ServerSettings),
}

/// The pure update function — the only writer of the model.
///
/// Toggling flips the voice-enabled flag in place. The terminal messages
/// (`Expired`, `Back`, `About`) persist the current settings exactly once,
/// mirroring the old save-on-exit semantics without an implicit write.
pub fn update(msg: VoiceSettingsMsg, model: &mut VoiceSettingsModel) -> Vec<VoiceSettingsEffect> {
    match msg {
        VoiceSettingsMsg::Start => Vec::new(),
        VoiceSettingsMsg::ToggleEnabled => {
            let current = model.settings.voice.enabled.unwrap_or(true);
            model.settings.voice.enabled = Some(!current);
            Vec::new()
        }
        VoiceSettingsMsg::Expired | VoiceSettingsMsg::Back | VoiceSettingsMsg::About => {
            vec![VoiceSettingsEffect::PersistSettings(model.settings.clone())]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(enabled: Option<bool>) -> VoiceSettingsModel {
        let mut settings = ServerSettings::default();
        settings.voice.enabled = enabled;
        VoiceSettingsModel::new(settings)
    }

    #[test]
    fn start_is_a_noop() {
        let mut m = model(Some(true));
        let effects = update(VoiceSettingsMsg::Start, &mut m);
        assert!(effects.is_empty());
        assert!(m.voice_enabled());
    }

    #[test]
    fn toggle_flips_enabled() {
        let mut m = model(Some(true));
        let effects = update(VoiceSettingsMsg::ToggleEnabled, &mut m);
        assert!(effects.is_empty());
        assert!(!m.voice_enabled());
    }

    #[test]
    fn toggle_defaults_to_enabled() {
        let mut m = model(None);
        update(VoiceSettingsMsg::ToggleEnabled, &mut m);
        assert!(!m.voice_enabled());
    }

    #[test]
    fn back_persists_current_settings() {
        let mut m = model(Some(false));
        let effects = update(VoiceSettingsMsg::Back, &mut m);
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            VoiceSettingsEffect::PersistSettings(s) => assert_eq!(s.voice.enabled, Some(false)),
        }
    }

    #[test]
    fn about_persists_current_settings() {
        let mut m = model(Some(true));
        let effects = update(VoiceSettingsMsg::About, &mut m);
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            VoiceSettingsEffect::PersistSettings(s) => assert_eq!(s.voice.enabled, Some(true)),
        }
    }

    #[test]
    fn expired_persists_current_settings() {
        let mut m = model(None);
        let effects = update(VoiceSettingsMsg::Expired, &mut m);
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            VoiceSettingsEffect::PersistSettings(s) => assert_eq!(s.voice.enabled, None),
        }
    }
}
