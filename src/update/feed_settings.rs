//! Pure update logic for feed settings.
//!
//! Holds the single source of truth for the feed settings view
//! (`FeedSettingsModel`), which absorbs the server's [`ServerSettings`] as
//! owned state — eliminating the raw `&mut ServerSettings` the old handler
//! carried alongside a separate `FeedSettingsModel`. Also holds the exhaustive
//! message vocabulary (`FeedSettingsMsg`) and the data-only effect vocabulary
//! (`FeedSettingsEffect`).
//!
//! Persistence is now an explicit effect, not an implicit save-on-exit: the
//! terminal messages (`Back`, `About`) and the shared lifecycle expiry
//! ([`Lifecycle::Expired`]) return a [`FeedSettingsEffect::PersistSettings`]
//! snapshot that the shell adapter executes when the host loop ends —
//! mirroring the voice settings migration.

use crate::entity::ServerSettings;
use crate::update::lifecycle::Lifecycle;

/// The feed settings view model — the single source of truth for the view.
#[derive(Debug, Clone)]
pub struct FeedSettingsModel {
    /// The server settings, including the feeds section being edited.
    pub(crate) settings: ServerSettings,
}

impl FeedSettingsModel {
    /// Constructs the model from the server settings loaded at boot.
    pub fn new(settings: ServerSettings) -> Self {
        Self { settings }
    }

    /// Whether feed notifications are currently enabled (defaults to enabled).
    pub fn is_enabled(&self) -> bool {
        self.settings.feeds.enabled.unwrap_or(true)
    }

    /// The configured notification channel id, if any.
    pub fn channel_id(&self) -> Option<String> {
        self.settings.feeds.channel_id.clone()
    }

    /// The configured subscribe-role id, if any.
    pub fn subscribe_role_id(&self) -> Option<String> {
        self.settings.feeds.subscribe_role_id.clone()
    }

    /// The configured unsubscribe-role id, if any.
    pub fn unsubscribe_role_id(&self) -> Option<String> {
        self.settings.feeds.unsubscribe_role_id.clone()
    }
}

/// Messages that drive the feed settings view.
///
/// Exhaustive: every way the world can change the feed settings model is one
/// variant. The lifecycle moments share the wrapped [`Lifecycle`] form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedSettingsMsg {
    /// The shared boot/timeout lifecycle moments.
    Lifecycle(Lifecycle),
    /// Toggle whether feed notifications are enabled.
    ToggleEnabled,
    /// Set the notification channel id (or clear it).
    SetChannel(Option<String>),
    /// Set the subscribe-permission role id (or clear it).
    SetSubRole(Option<String>),
    /// Set the unsubscribe-permission role id (or clear it).
    SetUnsubRole(Option<String>),
    /// The back button was pressed.
    Back,
    /// The about button was pressed.
    About,
}

impl From<Lifecycle> for FeedSettingsMsg {
    fn from(lifecycle: Lifecycle) -> Self {
        Self::Lifecycle(lifecycle)
    }
}

/// Effects the feed settings view can request.
///
/// Data-only: [`FeedSettingsEffect::PersistSettings`] carries a snapshot of the
/// settings to persist; the adapter performs the write.
#[derive(Debug, Clone)]
pub enum FeedSettingsEffect {
    /// Persist the current server settings (data-only snapshot).
    PersistSettings(ServerSettings),
}

/// The pure update function — the only writer of the model.
///
/// Each mutating message applies to `model.settings.feeds` in place. The
/// terminal exits — lifecycle expiry (through [`Lifecycle::handle`]), `Back`,
/// and `About` — persist the current settings exactly once, mirroring the old
/// save-on-exit semantics without an implicit write.
pub fn update(msg: FeedSettingsMsg, model: &mut FeedSettingsModel) -> Vec<FeedSettingsEffect> {
    match msg {
        FeedSettingsMsg::Lifecycle(lifecycle) => lifecycle.handle(|| persist(model)),
        FeedSettingsMsg::ToggleEnabled => {
            let current = model.settings.feeds.enabled.unwrap_or(true);
            model.settings.feeds.enabled = Some(!current);
            Vec::new()
        }
        FeedSettingsMsg::SetChannel(id) => {
            model.settings.feeds.channel_id = id;
            Vec::new()
        }
        FeedSettingsMsg::SetSubRole(id) => {
            model.settings.feeds.subscribe_role_id = id;
            Vec::new()
        }
        FeedSettingsMsg::SetUnsubRole(id) => {
            model.settings.feeds.unsubscribe_role_id = id;
            Vec::new()
        }
        FeedSettingsMsg::Back | FeedSettingsMsg::About => persist(model),
    }
}

/// The persist-on-exit behavior shared by expiry, `Back`, and `About`:
/// snapshot the current settings exactly once.
fn persist(model: &FeedSettingsModel) -> Vec<FeedSettingsEffect> {
    vec![FeedSettingsEffect::PersistSettings(model.settings.clone())]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(enabled: Option<bool>) -> FeedSettingsModel {
        let mut settings = ServerSettings::default();
        settings.feeds.enabled = enabled;
        FeedSettingsModel::new(settings)
    }

    #[test]
    fn start_is_a_noop() {
        let mut m = model(Some(true));
        let effects = update(FeedSettingsMsg::Lifecycle(Lifecycle::Start), &mut m);
        assert!(effects.is_empty());
        assert!(m.is_enabled());
    }

    #[test]
    fn toggle_flips_enabled() {
        let mut m = model(Some(true));
        let effects = update(FeedSettingsMsg::ToggleEnabled, &mut m);
        assert!(effects.is_empty());
        assert!(!m.is_enabled());
    }

    #[test]
    fn toggle_defaults_to_enabled() {
        let mut m = model(None);
        update(FeedSettingsMsg::ToggleEnabled, &mut m);
        assert!(!m.is_enabled());
    }

    #[test]
    fn set_channel_updates_id() {
        let mut m = model(Some(true));
        update(FeedSettingsMsg::SetChannel(Some("123".to_string())), &mut m);
        assert_eq!(m.channel_id(), Some("123".to_string()));
    }

    #[test]
    fn set_channel_none_clears_id() {
        let mut m = model(Some(true));
        update(FeedSettingsMsg::SetChannel(Some("123".to_string())), &mut m);
        update(FeedSettingsMsg::SetChannel(None), &mut m);
        assert_eq!(m.channel_id(), None);
    }

    #[test]
    fn set_sub_role_updates_id() {
        let mut m = model(Some(true));
        update(
            FeedSettingsMsg::SetSubRole(Some("role1".to_string())),
            &mut m,
        );
        assert_eq!(m.subscribe_role_id(), Some("role1".to_string()));
    }

    #[test]
    fn set_unsub_role_updates_id() {
        let mut m = model(Some(true));
        update(
            FeedSettingsMsg::SetUnsubRole(Some("role2".to_string())),
            &mut m,
        );
        assert_eq!(m.unsubscribe_role_id(), Some("role2".to_string()));
    }

    #[test]
    fn back_persists_current_settings() {
        let mut m = model(Some(false));
        let effects = update(FeedSettingsMsg::Back, &mut m);
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            FeedSettingsEffect::PersistSettings(s) => assert_eq!(s.feeds.enabled, Some(false)),
        }
    }

    #[test]
    fn about_persists_current_settings() {
        let mut m = model(Some(true));
        let effects = update(FeedSettingsMsg::About, &mut m);
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            FeedSettingsEffect::PersistSettings(s) => assert_eq!(s.feeds.enabled, Some(true)),
        }
    }

    #[test]
    fn expired_persists_current_settings() {
        let mut m = model(None);
        let effects = update(FeedSettingsMsg::Lifecycle(Lifecycle::Expired), &mut m);
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            FeedSettingsEffect::PersistSettings(s) => assert_eq!(s.feeds.enabled, None),
        }
    }
}
