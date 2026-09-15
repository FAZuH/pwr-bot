//! Shared test fixtures: plugin manifests and catalog entries.

#[path = "../tests/support/db.rs"]
pub(crate) mod db;

use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::CommandDef;
use pwr_plugin_protocol::Manifest;

use crate::plugin::CatalogEntry;

/// A manifest for a plugin named `name` carrying a single command named
/// `name`.
pub(crate) fn manifest_named(name: &str) -> Manifest {
    Manifest {
        name: name.to_string(),
        description: "test plugin".into(),
        version: "0.1.0".into(),
        commands: vec![CommandDef {
            create_command: serde_json::json!({"name": name, "description": "test"}),
        }],
        event_handlers: Vec::new(),
        tasks: Vec::new(),
        api_version: API_VERSION,
    }
}

/// A catalog entry for a plugin named `name` wrapping [`manifest_named`].
pub(crate) fn entry_named(name: &str) -> CatalogEntry {
    CatalogEntry {
        name: name.to_string(),
        url: format!("https://example.com/{name}"),
        sha256: "a".repeat(64),
        manifest: manifest_named(name),
        auto_enable: false,
    }
}
