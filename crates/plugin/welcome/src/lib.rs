//! The welcome panel plugin's declared surface: the name the hello
//! announces and the static manifest the host registers commands from.

use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::CommandDef;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::SettingsSection;
use serde_json::json;

pub mod image_generator;
pub mod repo;
pub mod storage;

/// The plugin's name: the hello `name` and the handle the host keeps it
/// under.
pub const PLUGIN_NAME: &str = "welcome";

/// The command the host Settings section dispatches.
pub const COMMAND_NAME: &str = "welcome-settings";

/// The user-facing slash command: the plugin owns its own name, so `/welcome`
/// is declared here rather than by a host Cog.
pub const SLASH_COMMAND_NAME: &str = "welcome";

/// Whether `cmd` is one of the names this panel answers — the Settings
/// section's `welcome-settings` or the `/welcome` slash command.
pub fn is_panel_command(cmd: Option<&str>) -> bool {
    matches!(cmd, Some(COMMAND_NAME) | Some(SLASH_COMMAND_NAME))
}

/// The plugin's static declaration, matching what its hello announces.
pub fn manifest() -> Manifest {
    Manifest {
        name: PLUGIN_NAME.into(),
        description: "Manage welcome card settings".into(),
        version: "0.1.0".into(),
        // The guild-only slash commands the host dispatches: a direct invoke
        // carries no `guild_id` in its re-parsed args, so the host injects the
        // invocation's guild into the args. No event handlers: an expiry
        // persists nothing, so there is no `view.timeout` work for a plugin
        // to do.
        commands: vec![
            CommandDef {
                create_command: json!({
                    "name": COMMAND_NAME,
                    "description": "Manage welcome card settings",
                    "dm_permission": false,
                }),
            },
            CommandDef {
                create_command: json!({
                    "name": SLASH_COMMAND_NAME,
                    "description": "Configure welcome cards for new members",
                    "dm_permission": false,
                }),
            },
        ],
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
