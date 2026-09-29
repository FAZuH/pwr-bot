use std::sync::Arc;

use base64::Engine as _;
use chrono::DateTime;
use chrono::Utc;
use pwr_ext::view;
use pwr_ext::view_support::ButtonStyle;
use pwr_plugin_protocol::ResolvedUser;
use pwr_plugin_protocol::RuntimeFile;
use pwr_plugin_protocol::ViewSpec;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;

use crate::GuildStatType;
use crate::TimeRange;
use crate::VoiceLeaderboardEntry;
use crate::VoiceLeaderboardTimeRange;
use crate::VoiceSettings;
use crate::VoiceStatsTimeRange;
use crate::entity::VoiceLeaderboardOptBuilder;
use crate::host_client::HostClient;
use crate::leaderboard::image_builder::LeaderboardImageBuilder;
use crate::service::VoiceTrackingService;
use crate::update::voice_leaderboard::LeaderboardData;
use crate::update::voice_leaderboard::VoiceLeaderboardEffect;
use crate::update::voice_leaderboard::VoiceLeaderboardModel;
use crate::update::voice_leaderboard::VoiceLeaderboardMsg;
use crate::update::voice_stats::VoiceStatsData;
use crate::update::voice_stats::VoiceStatsEffect;
use crate::update::voice_stats::VoiceStatsModel;
use crate::update::voice_stats::VoiceStatsMsg;
use crate::view::voice_leaderboard;
use crate::view::voice_stats;

const ADMINISTRATOR_PERMISSION: u64 = 1 << 3;
const MANAGE_GUILD_PERMISSION: u64 = 1 << 5;

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("you must be in a server to use this command")]
    GuildOnly,
    #[error("you need Manage Server or Administrator permission to configure voice settings")]
    PermissionDenied,
    #[error("invalid voice command argument: {0}")]
    InvalidArgument(String),
    #[error("voice service failed: {0}")]
    Service(String),
    #[error("identity lookup failed: {0}")]
    Identity(String),
    #[error("invalid voice plugin view state: {0}")]
    InvalidState(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorContext {
    pub user_id: u64,
    #[serde(default)]
    pub guild_id: Option<u64>,
    #[serde(default)]
    pub guild_name: Option<String>,
    #[serde(default)]
    pub member_roles: Vec<u64>,
    #[serde(default)]
    pub member_permissions: Option<u64>,
}

impl ActorContext {
    pub fn from_args(args: &Value) -> Result<Self, CommandError> {
        if let Some(context) = args.get("_context") {
            let mut actor: Self = serde_json::from_value(context.clone())
                .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
            if actor.guild_id.is_none() {
                actor.guild_id = id(args, "guild_id")?;
            }
            if actor.guild_name.is_none() {
                actor.guild_name = args
                    .get("guild_name")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            if actor.member_permissions.is_none() {
                actor.member_permissions = args
                    .get("member")
                    .and_then(|member| member.get("permissions"))
                    .and_then(|permissions| {
                        permissions
                            .as_u64()
                            .or_else(|| permissions.as_str().and_then(|value| value.parse().ok()))
                    });
            }
            return Ok(actor);
        }
        let user_id = args
            .get("user")
            .and_then(|user| user.get("id"))
            .and_then(|id| {
                id.as_u64()
                    .or_else(|| id.as_str().and_then(|id| id.parse().ok()))
            })
            .ok_or_else(|| CommandError::InvalidArgument("missing actor id".into()))?;
        let guild_id = args.get("guild_id").and_then(|id| {
            id.as_u64()
                .or_else(|| id.as_str().and_then(|id| id.parse().ok()))
        });
        let member_permissions = args
            .get("member")
            .and_then(|member| member.get("permissions"))
            .and_then(|permissions| {
                permissions
                    .as_u64()
                    .or_else(|| permissions.as_str().and_then(|value| value.parse().ok()))
            });
        let member_roles = args
            .get("member")
            .and_then(|member| member.get("roles"))
            .and_then(Value::as_array)
            .map(|roles| {
                roles
                    .iter()
                    .filter_map(|role| {
                        role.as_u64()
                            .or_else(|| role.as_str().and_then(|role| role.parse().ok()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            user_id,
            guild_id,
            guild_name: None,
            member_roles,
            member_permissions,
        })
    }

    pub fn require_admin(&self) -> Result<(), CommandError> {
        if self.member_permissions.unwrap_or_default()
            & (ADMINISTRATOR_PERMISSION | MANAGE_GUILD_PERMISSION)
            == 0
        {
            return Err(CommandError::PermissionDenied);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSession {
    pub guild_id: u64,
    pub settings: VoiceSettings,
}

pub fn settings_data(session: &SettingsSession) -> Value {
    let status_text = format!(
        "-# **Settings > Voice**\n## Voice Tracking Settings\n\n> 🛈  {}",
        if session.settings.enabled {
            "Voice tracking is **active**."
        } else {
            "Voice tracking is **paused**."
        }
    );
    let enabled_label = if session.settings.enabled {
        "Disable"
    } else {
        "Enable"
    };
    let enabled_style = if session.settings.enabled {
        ButtonStyle::Danger
    } else {
        ButtonStyle::Success
    };
    let message = view! {
        components_v2 {
            container {
                text_display { content: status_text }
                action_row {
                    button {
                        custom_id: "voice:toggle",
                        label: enabled_label,
                        style: enabled_style
                    }
                }
            }
            action_row {
                button {
                    custom_id: "voice:back",
                    label: "❮ Back",
                    style: ButtonStyle::Secondary
                }
                button {
                    custom_id: "voice:about",
                    label: "🛈 About",
                    style: ButtonStyle::Secondary
                }
            }
        }
    };
    serde_json::to_value(message).expect("voice settings view is serializable")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsSession {
    pub guild_id: u64,
    pub guild_name: String,
    pub author_id: u64,
    pub now: DateTime<Utc>,
    pub model: VoiceStatsModel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardSession {
    pub guild_id: u64,
    pub author_id: u64,
    pub model: VoiceLeaderboardModel,
}

pub fn stats_spec(session: &StatsSession) -> ViewSpec {
    ViewSpec {
        data: voice_stats::components(&session.model, session.now),
        ephemeral: false,
        view: serde_json::to_value(session).expect("voice stats session serializes"),
        files: session
            .model
            .image_bytes()
            .map(|bytes| runtime_file(voice_stats::IMAGE_FILENAME, bytes))
            .into_iter()
            .collect(),
    }
}

pub fn leaderboard_spec(session: &LeaderboardSession) -> ViewSpec {
    ViewSpec {
        data: voice_leaderboard::components(&session.model),
        ephemeral: false,
        view: serde_json::to_value(session).expect("voice leaderboard session serializes"),
        files: session
            .model
            .image_bytes()
            .map(|bytes| runtime_file(voice_leaderboard::IMAGE_FILENAME, bytes))
            .into_iter()
            .collect(),
    }
}

pub fn settings_spec(session: &SettingsSession) -> ViewSpec {
    ViewSpec {
        data: settings_data(session),
        ephemeral: false,
        view: serde_json::to_value(session).expect("voice settings session serializes"),
        files: vec![],
    }
}

fn runtime_file(filename: &str, bytes: &[u8]) -> RuntimeFile {
    RuntimeFile {
        filename: filename.into(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}

fn id(args: &Value, name: &str) -> Result<Option<u64>, CommandError> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
            .map(Some)
            .ok_or_else(|| CommandError::InvalidArgument(format!("`{name}` must be a Discord id"))),
    }
}

fn choice(args: &Value, name: &str) -> Option<String> {
    args.get(name).and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn target_user(args: &Value, actor: &ActorContext) -> Result<Option<u64>, CommandError> {
    let requested = id(args, "user")?;
    let Some(requested) = requested else {
        return Ok(None);
    };
    if requested == actor.user_id {
        return Ok(Some(requested));
    }
    if actor.guild_id.is_none() {
        return Err(CommandError::InvalidArgument(
            "in direct messages, you can only view your own voice stats".into(),
        ));
    }
    Ok(Some(requested))
}

fn stat_type(args: &Value) -> GuildStatType {
    match choice(args, "statistic").as_deref() {
        Some("Active Users") => GuildStatType::ActiveUserCount,
        Some("Total Time") => GuildStatType::TotalTime,
        _ => GuildStatType::AverageTime,
    }
}

fn stats_range(args: &Value) -> VoiceStatsTimeRange {
    match choice(args, "time_range").as_deref() {
        Some("Yearly") => VoiceStatsTimeRange::Yearly,
        Some("Monthly") => VoiceStatsTimeRange::Monthly,
        Some("Weekly") => VoiceStatsTimeRange::Weekly,
        Some("Hourly") => VoiceStatsTimeRange::Hourly,
        _ => VoiceStatsTimeRange::Monthly,
    }
}

fn leaderboard_range(args: &Value) -> VoiceLeaderboardTimeRange {
    match choice(args, "time_range").as_deref() {
        Some("Today") => VoiceLeaderboardTimeRange::Today,
        Some("Past 24 hours") => VoiceLeaderboardTimeRange::Past24Hours,
        Some("Past 72 hours") => VoiceLeaderboardTimeRange::Past72Hours,
        Some("Past 7 days") => VoiceLeaderboardTimeRange::Past7Days,
        Some("Past 14 days") => VoiceLeaderboardTimeRange::Past14Days,
        Some("This year") => VoiceLeaderboardTimeRange::ThisYear,
        Some("All time") => VoiceLeaderboardTimeRange::AllTime,
        _ => VoiceLeaderboardTimeRange::ThisMonth,
    }
}

async fn resolve_users(
    host: &Arc<dyn HostClient>,
    guild_id: Option<u64>,
    user_ids: &[u64],
) -> Result<Vec<ResolvedUser>, CommandError> {
    if user_ids.is_empty() {
        return Ok(Vec::new());
    }
    let response = host
        .call(
            "host.resolve_users",
            json!({ "guild_id": guild_id, "user_ids": user_ids }),
        )
        .await
        .map_err(|error| CommandError::Identity(error.to_string()))?;
    serde_json::from_value(response).map_err(|error| CommandError::Identity(error.to_string()))
}

async fn fetch_stats(
    service: &VoiceTrackingService,
    now: DateTime<Utc>,
    guild_id: u64,
    guild_name: String,
    user_id: Option<u64>,
    range: VoiceStatsTimeRange,
    stat: GuildStatType,
) -> Result<VoiceStatsData, CommandError> {
    let (since, until) = range.to_range(now);
    let raw_sessions = if range != VoiceStatsTimeRange::Yearly {
        service
            .get_sessions_in_range(guild_id, user_id, &since, &until)
            .await
            .map_err(|error| CommandError::Service(error.to_string()))?
    } else {
        Vec::new()
    };
    if let Some(user_id) = user_id {
        let activity = service
            .get_user_daily_activity(user_id, guild_id, &since, &until)
            .await
            .map_err(|error| CommandError::Service(error.to_string()))?;
        Ok(VoiceStatsData {
            guild_name,
            user_activity: activity,
            guild_stats: vec![],
            raw_sessions,
            target_user_name: None,
        })
    } else {
        let guild_stats = service
            .get_guild_daily_stats(guild_id, &since, &until, stat)
            .await
            .map_err(|error| CommandError::Service(error.to_string()))?;
        Ok(VoiceStatsData {
            guild_name,
            user_activity: vec![],
            guild_stats,
            raw_sessions,
            target_user_name: None,
        })
    }
}

pub async fn invoke_stats(
    service: &VoiceTrackingService,
    host: &Arc<dyn HostClient>,
    args: &Value,
) -> Result<StatsSession, CommandError> {
    let actor = ActorContext::from_args(args)?;
    let guild_id = actor.guild_id.ok_or(CommandError::GuildOnly)?;
    let now = Utc::now();
    let requested = target_user(args, &actor)?;
    let resolved = resolve_users(
        host,
        Some(guild_id),
        &requested.into_iter().collect::<Vec<_>>(),
    )
    .await?;
    if let Some(user) = resolved.first()
        && !user.is_member
    {
        return Err(CommandError::InvalidArgument(
            "the specified user is not a member of this server".into(),
        ));
    }
    let target_name = resolved.first().map(|user| user.name.clone());
    let range = stats_range(args);
    let stat = stat_type(args);
    let guild_name = actor
        .guild_name
        .clone()
        .unwrap_or_else(|| "Direct Messages".into());
    let data = fetch_stats(service, now, guild_id, guild_name, requested, range, stat).await?;
    let data = VoiceStatsData {
        target_user_name: target_name,
        ..data
    };
    let image_bytes = if data.user_activity.is_empty()
        && data.guild_stats.is_empty()
        && data.raw_sessions.is_empty()
    {
        None
    } else {
        Some(
            render_stats(now, range, stat, requested.is_some(), &data)
                .map_err(|error| CommandError::Service(error.to_string()))?,
        )
    };
    Ok(StatsSession {
        guild_id,
        guild_name: data.guild_name.clone(),
        author_id: actor.user_id,
        now,
        model: VoiceStatsModel::new(range, stat, requested, actor.user_id, data, image_bytes),
    })
}

fn render_stats(
    now: DateTime<Utc>,
    range: VoiceStatsTimeRange,
    stat: GuildStatType,
    is_user: bool,
    data: &VoiceStatsData,
) -> anyhow::Result<Vec<u8>> {
    voice_stats::generate_image(
        now,
        range,
        stat,
        is_user,
        &data.raw_sessions,
        &data.user_activity,
        &data.guild_stats,
    )
}

pub async fn interact_stats(
    service: &VoiceTrackingService,
    host: &Arc<dyn HostClient>,
    args: &Value,
) -> Result<StatsSession, CommandError> {
    let mut session: StatsSession = serde_json::from_value(
        args.get("view")
            .cloned()
            .ok_or_else(|| CommandError::InvalidState("missing stats view state".into()))?,
    )
    .map_err(|error| CommandError::InvalidState(error.to_string()))?;
    let now = Utc::now();
    session.now = now;
    let values = interaction_values(args);
    let custom_id = args
        .get("custom_id")
        .and_then(Value::as_str)
        .ok_or_else(|| CommandError::InvalidArgument("missing custom_id".into()))?;
    let message = voice_stats::translate(custom_id, &values, &session.model)
        .map_err(CommandError::InvalidArgument)?;
    let mut effects = crate::update::voice_stats::update(message, &mut session.model, now);
    while let Some(effect) = effects.pop() {
        match effect {
            VoiceStatsEffect::QueryStats {
                now: effect_now,
                time_range,
                stat_type,
                user_id,
            } => {
                let mut data = fetch_stats(
                    service,
                    effect_now,
                    session.guild_id,
                    session.guild_name.clone(),
                    user_id,
                    time_range,
                    stat_type,
                )
                .await?;
                if let Some(user_id) = user_id {
                    let profiles = resolve_users(host, Some(session.guild_id), &[user_id]).await?;
                    data.target_user_name = profiles.first().map(|user| user.name.clone());
                }
                effects.extend(crate::update::voice_stats::update(
                    VoiceStatsMsg::StatsLoaded {
                        data,
                        now: effect_now,
                    },
                    &mut session.model,
                    effect_now,
                ));
            }
            VoiceStatsEffect::RenderImage {
                now: effect_now,
                time_range,
                stat_type,
                is_user,
                raw_sessions,
                user_activity,
                guild_stats,
            } => {
                let data = VoiceStatsData {
                    guild_name: session.guild_name.clone(),
                    user_activity,
                    guild_stats,
                    raw_sessions,
                    target_user_name: session.model.display_name().to_string().into(),
                };
                let bytes = Some(
                    render_stats(effect_now, time_range, stat_type, is_user, &data)
                        .map_err(|error| CommandError::Service(error.to_string()))?,
                );
                effects.extend(crate::update::voice_stats::update(
                    VoiceStatsMsg::ImageRendered(bytes),
                    &mut session.model,
                    effect_now,
                ));
            }
        }
    }
    Ok(session)
}

pub async fn invoke_leaderboard(
    service: &VoiceTrackingService,
    host: &Arc<dyn HostClient>,
    args: &Value,
) -> Result<LeaderboardSession, CommandError> {
    let actor = ActorContext::from_args(args)?;
    let guild_id = actor.guild_id.ok_or(CommandError::GuildOnly)?;
    let now = Utc::now();
    let range = leaderboard_range(args);
    let entries = fetch_leaderboard(service, now, guild_id, range, false, None).await?;
    let mut model = VoiceLeaderboardModel::from_entries(entries, actor.user_id, 10).with_now(now);
    model.time_range = range;
    let image_bytes = render_leaderboard(host, guild_id, &model).await?;
    model = model.with_image_bytes(image_bytes);
    Ok(LeaderboardSession {
        guild_id,
        author_id: actor.user_id,
        model,
    })
}

async fn fetch_leaderboard(
    service: &VoiceTrackingService,
    now: DateTime<Utc>,
    guild_id: u64,
    range: VoiceLeaderboardTimeRange,
    partner: bool,
    target: Option<u64>,
) -> Result<Vec<VoiceLeaderboardEntry>, CommandError> {
    let (since, until) = range.to_range(now);
    let options = VoiceLeaderboardOptBuilder::default()
        .guild_id(guild_id)
        .limit(Some(u32::MAX))
        .since(Some(since))
        .until(Some(until))
        .build()
        .map_err(|error| CommandError::InvalidArgument(error.to_string()))?;
    if partner {
        service
            .get_partner_leaderboard(
                &options,
                target.ok_or_else(|| {
                    CommandError::InvalidArgument(
                        "a partner leaderboard needs a target user".into(),
                    )
                })?,
            )
            .await
            .map_err(|error| CommandError::Service(error.to_string()))
    } else {
        service
            .get_leaderboard_withopt(&options)
            .await
            .map_err(|error| CommandError::Service(error.to_string()))
    }
}

async fn render_leaderboard(
    host: &Arc<dyn HostClient>,
    guild_id: u64,
    model: &VoiceLeaderboardModel,
) -> Result<Option<Vec<u8>>, CommandError> {
    if model.is_empty() {
        return Ok(None);
    }
    let mut builder = LeaderboardImageBuilder::new(guild_id, Arc::clone(host));
    let entries = model.current_page_entries().to_vec();
    let offset = model.current_page_rank_offset();
    builder
        .build(&entries, offset)
        .await
        .map(|result| Some(result.image_bytes))
        .map_err(|error| CommandError::Service(error.to_string()))
}

pub async fn interact_leaderboard(
    service: &VoiceTrackingService,
    host: &Arc<dyn HostClient>,
    args: &Value,
) -> Result<LeaderboardSession, CommandError> {
    let mut session: LeaderboardSession = serde_json::from_value(
        args.get("view")
            .cloned()
            .ok_or_else(|| CommandError::InvalidState("missing leaderboard view state".into()))?,
    )
    .map_err(|error| CommandError::InvalidState(error.to_string()))?;
    let now = Utc::now();
    let _ = crate::update::voice_leaderboard::update(
        crate::update::voice_leaderboard::VoiceLeaderboardMsg::SetReferenceTime(now),
        &mut session.model,
    );
    let values = interaction_values(args);
    let custom_id = args
        .get("custom_id")
        .and_then(Value::as_str)
        .ok_or_else(|| CommandError::InvalidArgument("missing custom_id".into()))?;
    let message = voice_leaderboard::translate(custom_id, &values, &session.model)
        .map_err(CommandError::InvalidArgument)?;
    let mut effects = crate::update::voice_leaderboard::update(message, &mut session.model);
    while let Some(effect) = effects.pop() {
        match effect {
            VoiceLeaderboardEffect::QueryLeaderboard {
                now: effect_now,
                time_range,
                is_partner_mode,
                mut target_user_id,
            } => {
                let target_was_explicit = target_user_id.is_some();
                if is_partner_mode && target_user_id.is_none() {
                    target_user_id = Some(session.author_id);
                    session.model.target_user_id = target_user_id;
                }
                let entries = fetch_leaderboard(
                    service,
                    effect_now,
                    session.guild_id,
                    time_range,
                    is_partner_mode,
                    target_user_id,
                )
                .await?;
                let target_user_name = if is_partner_mode && target_was_explicit {
                    let target = target_user_id.ok_or_else(|| {
                        CommandError::InvalidState("partner target is missing".into())
                    })?;
                    resolve_users(host, Some(session.guild_id), &[target])
                        .await?
                        .first()
                        .map(|user| user.name.clone())
                } else {
                    None
                };
                effects.extend(crate::update::voice_leaderboard::update(
                    VoiceLeaderboardMsg::EntriesLoaded(LeaderboardData {
                        entries,
                        target_user_name,
                    }),
                    &mut session.model,
                ));
            }
            VoiceLeaderboardEffect::RenderImage {
                entries,
                rank_offset,
            } => {
                let bytes = if entries.is_empty() {
                    None
                } else {
                    let mut builder =
                        LeaderboardImageBuilder::new(session.guild_id, Arc::clone(host));
                    Some(
                        builder
                            .build(&entries, rank_offset)
                            .await
                            .map_err(|error| CommandError::Service(error.to_string()))?
                            .image_bytes,
                    )
                };
                effects.extend(crate::update::voice_leaderboard::update(
                    VoiceLeaderboardMsg::ImageRendered(bytes),
                    &mut session.model,
                ));
            }
        }
    }
    Ok(session)
}

pub fn interaction_values(args: &Value) -> Vec<String> {
    args.get("data")
        .and_then(|data| data.get("values"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| match value {
                    Value::String(value) => Some(value.clone()),
                    Value::Number(value) => Some(value.to_string()),
                    Value::Object(object) => object.get("id").and_then(|id| {
                        id.as_str()
                            .map(String::from)
                            .or_else(|| id.as_u64().map(|id| id.to_string()))
                    }),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_range_defaults_to_monthly() {
        assert_eq!(stats_range(&json!({})), VoiceStatsTimeRange::Monthly);
        assert_eq!(VoiceStatsTimeRange::default(), VoiceStatsTimeRange::Monthly);
    }

    #[test]
    fn stats_range_accepts_yearly() {
        assert_eq!(
            stats_range(&json!({ "time_range": "Yearly" })),
            VoiceStatsTimeRange::Yearly
        );
    }

    #[test]
    fn interaction_values_accepts_numeric_user_ids() {
        assert_eq!(
            interaction_values(&json!({
                "data": { "values": [{ "id": 42 }] }
            })),
            vec!["42"]
        );
    }
}
