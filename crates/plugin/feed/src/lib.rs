//! The feed plugin's declared command and settings surface.

use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::CommandDef;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::SettingsSection;
use serde_json::json;

pub mod command;
pub mod entity;
pub mod event;
pub mod feed;
pub mod host_client;
pub mod repo;
pub mod service;
pub mod storage;
pub mod subscriber;
pub mod task;
pub mod update;
pub mod view;

pub use feed::AniListPlatform;
pub use feed::BasePlatform;
pub use feed::ComickPlatform;
pub use feed::FeedItem;
pub use feed::FeedSource;
pub use feed::MangaDexPlatform;
pub use feed::Platform;
pub use feed::PlatformInfo;
pub use feed::Platforms;

/// The plugin's name: the hello `name` and the handle the host keeps it
/// under.
pub const PLUGIN_NAME: &str = "feed";

/// The direct settings command used by the host Settings section.
pub const COMMAND_NAME: &str = "feed-settings";

/// The public feed command root.
pub const FEED_COMMAND_NAME: &str = "feed";
pub const FEED_SETTINGS_COMMAND_NAME: &str = "feed settings";
pub const FEED_SUBSCRIBE_COMMAND_NAME: &str = "feed subscribe";
pub const FEED_UNSUBSCRIBE_COMMAND_NAME: &str = "feed unsubscribe";
pub const FEED_LIST_COMMAND_NAME: &str = "feed list";
pub const FEED_BATCH_COMMAND_NAME: &str = "feed batch";

fn send_into_option(name: &str, description: &str) -> serde_json::Value {
    json!({
        "name": name,
        "description": description,
        "type": 4,
        "required": false,
        "choices": [
            { "name": "Server", "value": 0 },
            { "name": "DM", "value": 1 },
        ],
    })
}

/// The plugin's static declaration, matching what its hello announces.
pub fn manifest() -> Manifest {
    let links_option = json!({
        "name": "links",
        "description": "Link(s) of the feeds. Separate links with commas (,)",
        "type": 3,
        "required": true,
        "autocomplete": true,
    });
    Manifest {
        name: PLUGIN_NAME.into(),
        description: "Manage feed subscriptions and settings".into(),
        version: "0.1.0".into(),
        commands: vec![
            CommandDef {
                create_command: json!({
                    "name": FEED_COMMAND_NAME,
                    "description": "Manage feed subscriptions and settings",
                    "options": [
                        {
                            "name": "settings",
                            "description": "Configure feed settings for this server",
                            "type": 1,
                            "default_member_permissions": "40",
                        },
                        {
                            "name": "subscribe",
                            "description": "Subscribe to one or more feeds",
                            "type": 1,
                            "options": [
                                links_option,
                                send_into_option(
                                    "send_into",
                                    "Where to send the notifications. Default to your DM",
                                ),
                            ],
                        },
                        {
                            "name": "unsubscribe",
                            "description": "Unsubscribe from one or more feeds",
                            "type": 1,
                            "options": [
                                links_option.clone(),
                                send_into_option(
                                    "send_into",
                                    "Where notifications were being sent. Default to DM",
                                ),
                            ],
                        },
                        {
                            "name": "list",
                            "description": "List your current feed subscriptions",
                            "type": 1,
                            "options": [send_into_option(
                                "sent_into",
                                "Where the notifications are being sent. Default to DM",
                            )],
                        },
                    ],
                }),
            },
            CommandDef {
                create_command: json!({
                    "name": COMMAND_NAME,
                    "description": "Manage feed subscription settings",
                    "dm_permission": false,
                    "guild_only": true,
                    "default_member_permissions": "40",
                }),
            },
        ],
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
