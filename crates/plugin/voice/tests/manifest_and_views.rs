use base64::Engine as _;
use chrono::DateTime;
use serde_json::json;
use voice::GuildStatType;
use voice::VoiceLeaderboardEntry;
use voice::VoiceSettings;
use voice::VoiceStatsTimeRange;
use voice::command::ActorContext;
use voice::command::CommandError;
use voice::command::LeaderboardSession;
use voice::command::SettingsSession;
use voice::command::StatsSession;
use voice::command::leaderboard_spec;
use voice::command::settings_data;
use voice::command::settings_spec;
use voice::command::stats_spec;
use voice::manifest;
use voice::update::voice_leaderboard::VoiceLeaderboardModel;
use voice::update::voice_stats::VoiceStatsData;
use voice::update::voice_stats::VoiceStatsModel;

#[test]
fn plugin_migrations_claim_only_voice_storage() {
    let initial = include_str!("../migrations/20260924-140100-0000_voice_owned_schema/up.sql");
    let sentinel = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("migrations/20260924-140000-0000_voice_storage");
    assert!(initial.contains("voice_sessions"));
    assert!(initial.contains("voice_settings"));
    assert!(initial.contains("voice_settings_import_state"));
    assert!(!sentinel.exists());
}

#[test]
fn manifest_keeps_the_vc_surface_and_declares_voice_events() {
    let manifest = manifest();

    assert_eq!(manifest.api_version, 2);
    assert_eq!(
        manifest.event_handlers,
        ["voice_state", "guild_create", "view.timeout"]
    );
    let names: Vec<&str> = manifest
        .commands
        .iter()
        .filter_map(|command| {
            command
                .create_command
                .get("name")
                .and_then(|name| name.as_str())
        })
        .collect();
    assert_eq!(names, ["vc", "voice-settings"]);
    let root = &manifest.commands[0].create_command;
    let options = root["options"].as_array().expect("vc options");
    assert_eq!(options[0]["name"], "settings");
    assert_eq!(options[0]["default_member_permissions"], "40");
    assert_eq!(options[1]["name"], "leaderboard");
    assert_eq!(options[2]["name"], "stats");
    let stats_options = options[2]["options"].as_array().expect("stats options");
    assert_eq!(
        stats_options[0]["description"],
        "Time period to display. Defaults to \"This month\""
    );
    assert_eq!(stats_options[0]["choices"][1]["name"], "Monthly");
    assert_eq!(stats_options[0]["choices"][1]["value"], 1);
}

#[test]
fn settings_gate_accepts_only_administrator_or_manage_guild() {
    let admin = ActorContext::from_args(&json!({
        "_context": { "user_id": 42, "member_permissions": 8 }
    }))
    .unwrap();
    let manager = ActorContext::from_args(&json!({
        "_context": { "user_id": 42, "member_permissions": 32 }
    }))
    .unwrap();
    let member = ActorContext::from_args(&json!({
        "_context": { "user_id": 42, "member_permissions": 0 }
    }))
    .unwrap();

    assert!(admin.require_admin().is_ok());
    assert!(manager.require_admin().is_ok());
    assert!(matches!(
        member.require_admin(),
        Err(CommandError::PermissionDenied)
    ));
}

#[test]
fn actor_context_falls_back_to_top_level_invocation_fields() {
    let actor = ActorContext::from_args(&json!({
        "_context": { "user_id": 42 },
        "guild_id": 7,
        "guild_name": "Guild",
        "member": { "permissions": 8 }
    }))
    .unwrap();

    assert_eq!(actor.user_id, 42);
    assert_eq!(actor.guild_id, Some(7));
    assert_eq!(actor.guild_name.as_deref(), Some("Guild"));
    assert!(actor.require_admin().is_ok());
}

#[test]
fn settings_view_uses_the_existing_panel_controls() {
    let enabled = SettingsSession {
        guild_id: 7,
        settings: VoiceSettings { enabled: true },
    };
    let data = settings_data(&enabled);
    assert_eq!(data["flags"], 32768);
    assert_eq!(
        data["components"][0]["components"][1]["components"][0]["custom_id"],
        "voice:toggle"
    );
    assert_eq!(data["components"][1]["components"][0]["label"], "❮ Back");
    assert_eq!(data["components"][1]["components"][1]["label"], "🛈 About");
}

#[test]
fn runtime_image_is_carried_by_the_view_spec() {
    let model = VoiceStatsModel::new(
        VoiceStatsTimeRange::Yearly,
        GuildStatType::AverageTime,
        None,
        42,
        VoiceStatsData {
            guild_name: "Guild".into(),
            user_activity: vec![],
            guild_stats: vec![],
            raw_sessions: vec![],
            target_user_name: None,
        },
        Some(vec![1, 2, 3]),
    );
    let spec = stats_spec(&StatsSession {
        guild_id: 7,
        guild_name: "Guild".into(),
        author_id: 42,
        now: DateTime::UNIX_EPOCH,
        model,
    });
    assert_eq!(spec.files[0].filename, "voice_stats.png");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(&spec.files[0].data_base64)
            .unwrap(),
        vec![1, 2, 3]
    );
}

#[test]
fn leaderboard_runtime_image_is_carried_by_the_view_spec() {
    let model = VoiceLeaderboardModel::from_entries(
        vec![VoiceLeaderboardEntry {
            user_id: 42,
            total_duration: 60,
        }],
        42,
        10,
    )
    .with_image_bytes(Some(vec![4, 5, 6]));
    let spec = leaderboard_spec(&LeaderboardSession {
        guild_id: 7,
        author_id: 42,
        model,
    });
    assert_eq!(spec.files[0].filename, "voice_leaderboard.jpg");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(&spec.files[0].data_base64)
            .unwrap(),
        vec![4, 5, 6]
    );
    assert_eq!(spec.data["flags"], 32768);
}

#[test]
fn toggle_defaults_to_enabled() {
    assert!(VoiceSettings::default().enabled);
}

#[test]
fn settings_session_round_trips_through_value() {
    let session = SettingsSession {
        guild_id: 7,
        settings: VoiceSettings { enabled: false },
    };
    let value = serde_json::to_value(&session).expect("serialize settings session");
    let parsed: SettingsSession = serde_json::from_value(value).expect("parse settings session");
    assert_eq!(parsed.guild_id, 7);
    assert!(!parsed.settings.enabled);
}

#[test]
fn malformed_settings_state_is_rejected() {
    assert!(serde_json::from_value::<SettingsSession>(json!({})).is_err());
    assert!(
        serde_json::from_value::<SettingsSession>(json!({
            "guild_id": 7,
            "settings": {}
        }))
        .is_err()
    );
}

#[test]
fn panel_is_components_v2_without_legacy_content() {
    let data = settings_data(&SettingsSession {
        guild_id: 7,
        settings: VoiceSettings::default(),
    });
    assert_eq!(data["flags"], 32768);
    assert!(data.get("content").is_none());
}

#[test]
fn panel_mirrors_the_monolith_layout() {
    let data = settings_data(&SettingsSession {
        guild_id: 7,
        settings: VoiceSettings::default(),
    });
    let components = data["components"].as_array().expect("components");
    assert_eq!(components.len(), 2);
    let children = components[0]["components"].as_array().expect("children");
    assert_eq!(children.len(), 2);
    assert!(
        children[0]["content"]
            .as_str()
            .expect("status text")
            .contains("Voice tracking is **active**")
    );
    assert_eq!(children[1]["components"][0]["custom_id"], "voice:toggle");
    assert_eq!(components[1]["components"][0]["custom_id"], "voice:back");
    assert_eq!(components[1]["components"][1]["custom_id"], "voice:about");
}

#[test]
fn paused_panel_renders_the_paused_copy_and_enable_button() {
    let data = settings_data(&SettingsSession {
        guild_id: 7,
        settings: VoiceSettings { enabled: false },
    });
    let children = data["components"][0]["components"]
        .as_array()
        .expect("children");
    assert!(
        children[0]["content"]
            .as_str()
            .expect("status text")
            .contains("Voice tracking is **paused**")
    );
    assert_eq!(children[1]["components"][0]["label"], "Enable");
    assert_eq!(children[1]["components"][0]["style"], 3);
}

#[test]
fn get_and_update_args_carry_the_guild_and_actor() {
    let actor = ActorContext::from_args(&json!({
        "_context": {
            "user_id": 42,
            "guild_id": 7,
            "member_permissions": 8
        }
    }))
    .unwrap();
    assert_eq!(actor.guild_id, Some(7));
    assert!(actor.require_admin().is_ok());
}

#[test]
fn settings_spec_keeps_the_session_state_without_a_file() {
    let spec = settings_spec(&SettingsSession {
        guild_id: 7,
        settings: VoiceSettings { enabled: false },
    });
    assert!(spec.files.is_empty());
    assert_eq!(
        spec.view,
        json!({"guild_id": 7, "settings": {"enabled": false}})
    );
}
