//! The welcome panel plugin's declared surface: the name the hello
//! announces and the static manifest the host registers commands from.

use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::SettingsSection;

pub mod image_generator;
pub mod repo;
pub mod storage;

/// The plugin's name: the hello `name` and the handle the host keeps it
/// under.
pub const PLUGIN_NAME: &str = "welcome";

/// The panel's invoke command. Not a slash command: the host dispatches it
/// from the Settings section's tile, so the panel is opened only by
/// `/settings welcome`, which parks a host Settings session behind it. A
/// panel opened any other way has no session waiting, and its Back and About
/// have nowhere to hand the message back to.
pub const COMMAND_NAME: &str = "welcome-settings";

/// Whether `cmd` is the name this panel answers.
pub fn is_panel_command(cmd: Option<&str>) -> bool {
    cmd == Some(COMMAND_NAME)
}

/// The plugin's static declaration, matching what its hello announces.
pub fn manifest() -> Manifest {
    Manifest {
        name: PLUGIN_NAME.into(),
        description: "Manage welcome card settings".into(),
        version: "0.1.0".into(),
        // No slash command. The panel is reached through the host's
        // `/settings` and its section tile, so there is nothing to register
        // with Discord and nothing to sync when the panel comes and goes.
        // No event handlers either: an expiry persists nothing, so there is
        // no `view.timeout` work for a plugin to do.
        commands: vec![],
        event_handlers: vec![],
        tasks: vec![],
        settings: vec![SettingsSection {
            name: "Welcome".into(),
            description: "Manage welcome card settings".into(),
            command: COMMAND_NAME.into(),
        }],
        requires: vec![],
        api_version: API_VERSION,
    }
}
