//! Plugin manifest: the declaration a plugin carries at handshake.
//!
//! The manifest is not a wire envelope message; it is the plugin's static
//! declaration of what it offers (commands, event subscriptions, tasks) and
//! which protocol version it speaks. The host validates it when the plugin
//! announces itself and rejects the handshake on failure.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::msg::API_VERSION;

/// A plugin's declaration, carried alongside the hello handshake. Every field
/// is required; a manifest missing one fails to deserialize.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// Plugin name, e.g. `feed`.
    pub name: String,
    /// One-line human-readable description.
    pub description: String,
    /// Plugin version, free-form (e.g. `0.1.0`).
    pub version: String,
    /// Slash commands the plugin serves. Each entry is a serialized Discord
    /// `CreateCommand` JSON blob — the single source of truth for both Discord
    /// registration and host-side argument re-parsing.
    pub commands: Vec<CommandDef>,
    /// Event names the plugin subscribes to, e.g. `voice_state`.
    pub event_handlers: Vec<String>,
    /// Recurring tasks the host should drive.
    pub tasks: Vec<TaskDef>,
    /// Settings sections the plugin contributes to the host `/settings`
    /// surface; each targets one of the plugin's own commands.
    #[serde(default)]
    pub settings: Vec<SettingsSection>,
    /// Protocol version this manifest is written for; validated against
    /// [`API_VERSION`].
    pub api_version: u32,
}

impl Manifest {
    /// Validates the manifest against this host: the `api_version` must equal
    /// [`API_VERSION`] and every command blob must be a JSON object carrying at
    /// least `name` and `description` strings.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.api_version != API_VERSION {
            return Err(ManifestError::UnsupportedApiVersion {
                got: self.api_version,
                expected: API_VERSION,
            });
        }
        for (index, command) in self.commands.iter().enumerate() {
            validate_command_blob(index, &command.create_command)?;
        }
        for (index, section) in self.settings.iter().enumerate() {
            validate_settings_section(index, section, &self.commands)?;
        }
        Ok(())
    }
}

/// A single slash command declaration: a raw Discord-native `CreateCommand`
/// JSON blob. Kept as an opaque [`Value`] — a full `CreateCommand` schema is
/// Discord-owned and huge — validated minimally at handshake (see
/// [`Manifest::validate`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandDef {
    /// The `CreateCommand` blob, e.g. `{"name":"feed.list","description":...}`.
    pub create_command: Value,
}

/// A recurring task the host drives on an interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDef {
    /// Task name, e.g. `prune`.
    pub name: String,
    /// Interval between runs, in seconds.
    pub interval_secs: u64,
    /// Command to invoke, e.g. `feed.prune`.
    pub command: String,
}

/// A settings section a plugin contributes to the host `/settings` surface:
/// the tile's display data and the plugin command invoking it opens the
/// section's panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsSection {
    /// Section title, e.g. `Feed`.
    pub name: String,
    /// One-line human-readable description shown on the tile.
    pub description: String,
    /// The plugin command invoking this section opens; must name one of the
    /// manifest's own commands (see [`Manifest::validate`]).
    pub command: String,
}

/// Why a [`Manifest`] failed validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// The manifest was written for a different wire protocol version.
    #[error("unsupported api_version {got}; this host speaks {expected}")]
    UnsupportedApiVersion {
        /// Version found in the manifest.
        got: u32,
        /// Version this host speaks.
        expected: u32,
    },
    /// A command entry is not a valid `CreateCommand` blob.
    #[error("command {index} is not valid CreateCommand JSON: {reason}")]
    InvalidCommand {
        /// Index of the offending entry in `commands`.
        index: usize,
        /// Why the entry failed, e.g. `name` must be a string.
        reason: String,
    },
    /// A settings section does not name one of the manifest's own commands.
    #[error("settings section {index} is invalid: {reason}")]
    InvalidSettings {
        /// Index of the offending entry in `settings`.
        index: usize,
        /// Why the entry failed.
        reason: String,
    },
}

/// Validates one `CreateCommand` blob: must be a JSON object containing at
/// least `name` and `description` strings. Additional fields (options, etc.)
/// are passed through verbatim.
fn validate_command_blob(index: usize, blob: &Value) -> Result<(), ManifestError> {
    let object = blob
        .as_object()
        .ok_or_else(|| ManifestError::InvalidCommand {
            index,
            reason: "not a JSON object".into(),
        })?;
    if !matches!(object.get("name"), Some(Value::String(_))) {
        return Err(ManifestError::InvalidCommand {
            index,
            reason: "`name` must be a string".into(),
        });
    }
    if !matches!(object.get("description"), Some(Value::String(_))) {
        return Err(ManifestError::InvalidCommand {
            index,
            reason: "`description` must be a string".into(),
        });
    }
    Ok(())
}

/// Validates one settings section: its `command` must name a command the
/// manifest itself declares — a section pointing outside the plugin has no
/// invocable target.
fn validate_settings_section(
    index: usize,
    section: &SettingsSection,
    commands: &[CommandDef],
) -> Result<(), ManifestError> {
    let declared = commands.iter().any(|command| {
        command
            .create_command
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name == section.command)
    });
    if declared {
        Ok(())
    } else {
        Err(ManifestError::InvalidSettings {
            index,
            reason: format!(
                "`command` `{}` is not one of the manifest's commands",
                section.command
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn sample_manifest() -> Manifest {
        Manifest {
            name: "feed".into(),
            description: "Feed subscriptions".into(),
            version: "0.1.0".into(),
            commands: vec![CommandDef {
                create_command: json!({
                    "name": "feed.list",
                    "description": "List feeds",
                    "options": []
                }),
            }],
            event_handlers: vec!["voice_state".into()],
            tasks: vec![TaskDef {
                name: "prune".into(),
                interval_secs: 3600,
                command: "feed.prune".into(),
            }],
            settings: vec![],
            api_version: API_VERSION,
        }
    }

    // ── serialization ────────────────────────────────────────────────────────

    #[test]
    fn manifest_serializes_to_declared_shape() {
        assert_eq!(
            serde_json::to_string(&sample_manifest()).unwrap(),
            r#"{"name":"feed","description":"Feed subscriptions","version":"0.1.0","commands":[{"create_command":{"description":"List feeds","name":"feed.list","options":[]}}],"event_handlers":["voice_state"],"tasks":[{"name":"prune","interval_secs":3600,"command":"feed.prune"}],"settings":[],"api_version":2}"#
        );
    }

    #[test]
    fn manifest_round_trips_losslessly() {
        let manifest = sample_manifest();
        let json = serde_json::to_string(&manifest).unwrap();
        assert_eq!(serde_json::from_str::<Manifest>(&json).unwrap(), manifest);
    }

    #[test]
    fn missing_required_field_fails_to_deserialize() {
        let json = r#"{"name":"feed","description":"d","version":"0.1.0","commands":[],"event_handlers":[],"api_version":3}"#;
        assert!(serde_json::from_str::<Manifest>(json).is_err());
    }

    #[test]
    fn command_blob_is_passed_through_verbatim() {
        let manifest = sample_manifest();
        assert_eq!(
            manifest.commands[0].create_command,
            json!({"name": "feed.list", "description": "List feeds", "options": []})
        );
    }

    // ── api_version gating ───────────────────────────────────────────────────

    #[test]
    fn current_api_version_validates() {
        assert_eq!(sample_manifest().validate(), Ok(()));
    }

    #[test]
    fn unknown_api_version_is_rejected() {
        let mut manifest = sample_manifest();
        manifest.api_version = 3;
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::UnsupportedApiVersion {
                got: 3,
                expected: API_VERSION,
            })
        );
    }

    // ── command blob schema ──────────────────────────────────────────────────

    #[test]
    fn command_blob_with_name_and_description_validates() {
        let mut manifest = sample_manifest();
        manifest.commands = vec![CommandDef {
            create_command: json!({"name": "ping", "description": "Pong"}),
        }];
        assert_eq!(manifest.validate(), Ok(()));
    }

    #[test]
    fn command_blob_with_extra_fields_validates() {
        let mut manifest = sample_manifest();
        manifest.commands = vec![CommandDef {
            create_command: json!({
                "name": "say",
                "description": "Echo text",
                "options": [{"name": "text", "description": "What to say", "kind": 3, "required": true}]
            }),
        }];
        assert_eq!(manifest.validate(), Ok(()));
    }

    #[test]
    fn command_blob_missing_name_is_rejected() {
        let mut manifest = sample_manifest();
        manifest.commands = vec![CommandDef {
            create_command: json!({"description": "no name"}),
        }];
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::InvalidCommand {
                index: 0,
                reason: "`name` must be a string".into(),
            })
        );
    }

    #[test]
    fn command_blob_with_non_string_name_is_rejected() {
        let mut manifest = sample_manifest();
        manifest.commands = vec![CommandDef {
            create_command: json!({"name": 42, "description": "numeric name"}),
        }];
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::InvalidCommand {
                index: 0,
                reason: "`name` must be a string".into(),
            })
        );
    }

    #[test]
    fn command_blob_missing_description_is_rejected() {
        let mut manifest = sample_manifest();
        manifest.commands = vec![CommandDef {
            create_command: json!({"name": "no-desc"}),
        }];
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::InvalidCommand {
                index: 0,
                reason: "`description` must be a string".into(),
            })
        );
    }

    #[test]
    fn command_blob_that_is_not_an_object_is_rejected() {
        let mut manifest = sample_manifest();
        manifest.commands = vec![CommandDef {
            create_command: json!("feed.list"),
        }];
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::InvalidCommand {
                index: 0,
                reason: "not a JSON object".into(),
            })
        );
    }

    #[test]
    fn validation_reports_the_offending_index() {
        let mut manifest = sample_manifest();
        manifest.commands = vec![
            CommandDef {
                create_command: json!({"name": "ok", "description": "fine"}),
            },
            CommandDef {
                create_command: json!({"name": "broken"}),
            },
        ];
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::InvalidCommand {
                index: 1,
                reason: "`description` must be a string".into(),
            })
        );
    }

    // ── settings sections ────────────────────────────────────────────────────

    #[test]
    fn manifest_without_settings_deserializes_with_an_empty_list() {
        let json = r#"{"name":"feed","description":"d","version":"0.1.0","commands":[],"event_handlers":[],"tasks":[],"api_version":2}"#;
        let manifest: Manifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.settings, vec![]);
        assert_eq!(manifest.validate(), Ok(()));
    }

    #[test]
    fn settings_section_targeting_a_declared_command_validates() {
        let mut manifest = sample_manifest();
        manifest.settings = vec![SettingsSection {
            name: "Feeds".into(),
            description: "Manage feed subscriptions".into(),
            command: "feed.list".into(),
        }];
        assert_eq!(manifest.validate(), Ok(()));
    }

    #[test]
    fn settings_section_targeting_an_undeclared_command_is_rejected() {
        let mut manifest = sample_manifest();
        manifest.settings = vec![SettingsSection {
            name: "Feeds".into(),
            description: "Manage feed subscriptions".into(),
            command: "feed.add".into(),
        }];
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::InvalidSettings {
                index: 0,
                reason: "`command` `feed.add` is not one of the manifest's commands".into(),
            })
        );
    }

    #[test]
    fn validation_reports_the_offending_settings_index() {
        let mut manifest = sample_manifest();
        manifest.settings = vec![
            SettingsSection {
                name: "Feeds".into(),
                description: "Fine".into(),
                command: "feed.list".into(),
            },
            SettingsSection {
                name: "Broken".into(),
                description: "No target".into(),
                command: "ghost".into(),
            },
        ];
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::InvalidSettings {
                index: 1,
                reason: "`command` `ghost` is not one of the manifest's commands".into(),
            })
        );
    }
}
