//! The feed panel plugin's declared surface: the name the hello announces
//! and the static manifest the host registers commands from.

use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::CommandDef;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::SettingsSection;
use serde_json::json;

/// The plugin's name: the hello `name` and the handle the host keeps it
/// under.
pub const PLUGIN_NAME: &str = "feed";

/// The command the panel serves: the manifest's slash command and the
/// invoke command the host Settings section and the `/feed settings`
/// deep-link dispatch.
pub const COMMAND_NAME: &str = "feed-settings";

/// The plugin's static declaration, matching what its hello announces.
pub fn manifest() -> Manifest {
    Manifest {
        name: PLUGIN_NAME.into(),
        description: "Manage feed subscription settings".into(),
        version: "0.1.0".into(),
        // The guild-only slash command the host Settings section dispatches:
        // a direct invoke carries no `guild_id` in its re-parsed args, so
        // the host injects the invocation's guild into the args.
        commands: vec![CommandDef {
            create_command: json!({
                "name": COMMAND_NAME,
                "description": "Manage feed subscription settings",
                "dm_permission": false,
            }),
        }],
        event_handlers: vec!["view.timeout".into()],
        tasks: vec![],
        settings: vec![SettingsSection {
            name: "Feed".into(),
            description: "Manage feed subscription settings".into(),
            command: COMMAND_NAME.into(),
        }],
        api_version: API_VERSION,
    }
}
