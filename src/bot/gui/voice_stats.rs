//! The `/voice stats` feature shell — a [`GuiFeature`] over the pure voice
//! stats core.
//!
//! Renders the stats summary, the contribution/line-chart media gallery, the
//! time-range and stat-type button rows, and the optional user-select row.
//! The initial data and image arrive at model construction (data-in via
//! `Config`); in-session refetches are [`VoiceStatsEffect`]s executed by the
//! [`VoiceStatsEffectHandler`] adapter, whose results flow back as
//! `StatsLoaded` / `ImageRendered` messages.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use chrono::NaiveDate;
use contribution_grid::ContributionGraph;
use contribution_grid::builtins::Strategy;
use contribution_grid::builtins::Theme;
use pwr_ext::component;

use crate::bot::command::prelude::*;
use crate::bot::command::voice::GuildStatType;
use crate::bot::command::voice::TimeRange;
use crate::bot::command::voice::VoiceStatsTimeRange;
use crate::bot::command::voice::stats::chart::generate_line_chart;
use crate::bot::gui::effects::EffectHandler;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::feature::sealed;
use crate::entity::GuildDailyStats;
use crate::entity::VoiceDailyActivity;
use crate::entity::VoiceSessionsEntity;
use crate::service::traits::VoiceTracker;
use crate::update::voice_stats::VoiceStatsData;
use crate::update::voice_stats::VoiceStatsEffect;
use crate::update::voice_stats::VoiceStatsModel;
use crate::update::voice_stats::VoiceStatsMsg;
use crate::update::voice_stats::update as voice_stats_update;

/// Filename for the voice stats image attachment.
pub const VOICE_STATS_IMAGE_FILENAME: &str = "voice_stats.png";

/// Data-in for the voice stats feature: the loaded snapshot and its image.
pub struct VoiceStatsConfig {
    pub time_range: VoiceStatsTimeRange,
    pub stat_type: GuildStatType,
    pub user_id: Option<u64>,
    pub fallback_user_id: u64,
    pub data: VoiceStatsData,
    pub image_bytes: Option<Vec<u8>>,
}

action_enum! {
    VoiceStatsAction {
        #[label = "Yearly"]
        TimeYearly,
        #[label = "Monthly"]
        TimeMonthly,
        #[label = "Weekly"]
        TimeWeekly,
        #[label = "Hourly"]
        TimeHourly,

        #[label = "Unique Users"]
        StatUniqueUsers,
        #[label = "Total Time"]
        StatTotalTime,
        #[label = "Average Time"]
        StatAverageTime,

        ToggleDataMode,
        SelectUser,
    }
}

/// The voice stats feature.
pub struct VoiceStatsFeature;

impl sealed::Sealed for VoiceStatsFeature {}

impl GuiFeature for VoiceStatsFeature {
    type Model = VoiceStatsModel;
    type Msg = VoiceStatsMsg;
    type Action = VoiceStatsAction;
    type Effect = VoiceStatsEffect;
    type Config = VoiceStatsConfig;

    fn initial(config: Self::Config) -> Self::Model {
        VoiceStatsModel::new(
            config.time_range,
            config.stat_type,
            config.user_id,
            config.fallback_user_id,
            config.data,
            config.image_bytes,
        )
    }

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Vec<Self::Effect> {
        voice_stats_update(msg, model)
    }

    fn view<'a>(
        model: &'a Self::Model,
        registry: &mut ActionRegistry<Self::Action>,
    ) -> Vec<CreateComponent<'a>> {
        use VoiceStatsAction::*;

        // Keep the original registration order so the component order stays
        // byte-identical for the render snapshots in this module.
        let toggle = registry.register(ToggleDataMode);
        let time_yearly = registry.register(TimeYearly);
        let time_monthly = registry.register(TimeMonthly);
        let time_weekly = registry.register(TimeWeekly);
        let time_hourly = registry.register(TimeHourly);
        let stat_unique = if !model.is_user_stats() {
            Some(registry.register(StatUniqueUsers))
        } else {
            None
        };
        let stat_total = registry.register(StatTotalTime);
        let stat_avg = registry.register(StatAverageTime);
        let user_select = if model.is_user_stats() {
            Some(registry.register(SelectUser))
        } else {
            None
        };

        let toggle_label = if model.is_user_stats() {
            "Show server stats"
        } else {
            "Show user stats"
        };

        let focus_component = if model.user_activity().is_empty() && model.guild_stats().is_empty()
        {
            CreateContainerComponent::TextDisplay(component! {
                text_display {
                    content: "No voice activity recorded for this time range.\n\nJoin a **voice channel** to start tracking!"
                }
            })
        } else {
            CreateContainerComponent::MediaGallery(component! {
                media_gallery {
                    media_gallery_item {
                        media: format!("attachment://{VOICE_STATS_IMAGE_FILENAME}")
                    }
                }
            })
        };

        let container = CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(component! {
                text_display { content: format_stats_summary(model) }
            }),
            CreateContainerComponent::Separator(component! {
                separator { divider: true }
            }),
            focus_component,
            CreateContainerComponent::ActionRow(component! {
                action_row {
                    button {
                        custom_id: toggle.id,
                        label: toggle_label,
                        style: ButtonStyle::Primary
                    }
                }
            }),
        ]);

        let mut components = vec![CreateComponent::Container(container)];

        components.push(CreateComponent::ActionRow(component! {
            action_row {
                button {
                    custom_id: time_yearly.id,
                    label: time_yearly.label,
                    style: if model.time_range() == VoiceStatsTimeRange::Yearly {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    }
                }
                button {
                    custom_id: time_monthly.id,
                    label: time_monthly.label,
                    style: if model.time_range() == VoiceStatsTimeRange::Monthly {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    }
                }
                button {
                    custom_id: time_weekly.id,
                    label: time_weekly.label,
                    style: if model.time_range() == VoiceStatsTimeRange::Weekly {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    }
                }
                button {
                    custom_id: time_hourly.id,
                    label: time_hourly.label,
                    style: if model.time_range() == VoiceStatsTimeRange::Hourly {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    }
                }
            }
        }));

        let mut stat_buttons = vec![];
        if let Some(unique) = &stat_unique {
            stat_buttons.push(unique.clone().as_button().style(
                if model.stat_type() == GuildStatType::ActiveUserCount {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Secondary
                },
            ));
        }
        stat_buttons.push(stat_total.clone().as_button().style(
            if model.stat_type() == GuildStatType::TotalTime {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Secondary
            },
        ));
        stat_buttons.push(stat_avg.clone().as_button().style(
            if model.stat_type() == GuildStatType::AverageTime {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Secondary
            },
        ));
        components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
            stat_buttons.into(),
        )));

        if model.is_user_stats() {
            let select = user_select.as_ref().expect("user select registered");
            let user_kind = CreateSelectMenuKind::User {
                default_users: Some(Cow::Owned(vec![UserId::new(
                    model.user_id().expect("user mode has a user id"),
                )])),
            };
            components.push(CreateComponent::ActionRow(component! {
                action_row {
                    select_menu {
                        custom_id: select.id.clone(),
                        kind: user_kind
                    }
                }
            }));
        }

        components
    }

    fn translate(
        action: &Self::Action,
        values: SelectValues,
        _model: &Self::Model,
    ) -> Option<Self::Msg> {
        match action {
            VoiceStatsAction::TimeYearly => {
                Some(VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Yearly))
            }
            VoiceStatsAction::TimeMonthly => {
                Some(VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Monthly))
            }
            VoiceStatsAction::TimeWeekly => {
                Some(VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Weekly))
            }
            VoiceStatsAction::TimeHourly => {
                Some(VoiceStatsMsg::ChangeTimeRange(VoiceStatsTimeRange::Hourly))
            }
            VoiceStatsAction::StatUniqueUsers => Some(VoiceStatsMsg::ChangeStatType(
                GuildStatType::ActiveUserCount,
            )),
            VoiceStatsAction::StatTotalTime => {
                Some(VoiceStatsMsg::ChangeStatType(GuildStatType::TotalTime))
            }
            VoiceStatsAction::StatAverageTime => {
                Some(VoiceStatsMsg::ChangeStatType(GuildStatType::AverageTime))
            }
            VoiceStatsAction::ToggleDataMode => Some(VoiceStatsMsg::ToggleDataMode),
            VoiceStatsAction::SelectUser => {
                let user_id = match values {
                    SelectValues::User(v) => v.first().map(|id| id.get()),
                    _ => None,
                };
                Some(VoiceStatsMsg::SetUser(user_id))
            }
        }
    }

    fn attachments(model: &Self::Model) -> Vec<CreateAttachment<'static>> {
        model
            .image_bytes()
            .map(|bytes| CreateAttachment::bytes(bytes.to_vec(), VOICE_STATS_IMAGE_FILENAME))
            .into_iter()
            .collect()
    }
}

/// Formats the stats summary text shown in the leading text display.
fn format_stats_summary(model: &VoiceStatsModel) -> String {
    let (since, until) = model.time_range().to_range();
    let time_range_text = format!(
        "-# Time Range: **{}** — <t:{}:f> to <t:{}:R>",
        model.time_range().display_name(),
        since.timestamp(),
        until.timestamp(),
    );

    if model.is_user_stats() {
        let total = format_duration(model.total_time());
        let avg = format_duration(model.average_daily_time());
        let streak = model.current_streak();

        format!(
            "### Voice Stats\n{}\n\n**User:** {}\n**Total Time:** {}\n**Average Daily:** {}\n**Current Streak:** {} day(s)",
            time_range_text,
            model.display_name(),
            total,
            avg,
            streak
        )
    } else {
        let (first_label, first_value, second_label, second_value) = match model.stat_type() {
            GuildStatType::AverageTime => {
                let peak = model.guild_stats().iter().max_by_key(|s| s.value);
                let peak_str = peak
                    .map(|s| format_duration(s.value))
                    .unwrap_or_else(|| "None".to_string());
                let peak_day = peak
                    .map(|s| s.day)
                    .unwrap_or_else(|| chrono::Utc::now().date_naive());
                let peak_day_str = format!(
                    " {} on <t:{}:d>",
                    peak_str,
                    peak_day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()
                );

                ("Peak Time", peak_str, "Most Active", peak_day_str)
            }
            GuildStatType::ActiveUserCount => {
                let active_users = if model.guild_stats().is_empty() {
                    0
                } else {
                    let total_days = model.guild_stats().len() as i64;
                    (model.total_active_users() as f64 / total_days as f64).ceil() as i64
                };

                let peak = model.guild_stats().iter().max_by_key(|s| s.value);
                let peak_str = peak
                    .map(|s| s.value.to_string())
                    .unwrap_or_else(|| "None".to_string());
                let peak_day = peak
                    .map(|s| s.day)
                    .unwrap_or_else(|| chrono::Utc::now().date_naive());
                let peak_day_str = format!(
                    " {} on <t:{}:d>",
                    peak_str,
                    peak_day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()
                );

                (
                    "Avg Daily Users",
                    active_users.to_string(),
                    "Most Active",
                    peak_day_str,
                )
            }
            GuildStatType::TotalTime => {
                let peak = model.guild_stats().iter().max_by_key(|s| s.value);
                let peak_str = peak
                    .map(|s| format_duration(s.value))
                    .unwrap_or_else(|| "None".to_string());
                let peak_day = peak
                    .map(|s| s.day)
                    .unwrap_or_else(|| chrono::Utc::now().date_naive());
                let peak_day_str = format!(
                    " {} on <t:{}:d>",
                    peak_str,
                    peak_day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()
                );

                ("Peak Total Time", peak_str, "Most Active", peak_day_str)
            }
        };

        format!(
            "### Voice Stats\n{}\n\n**Server:** {}\n**{}:** {}\n**{}:**{}",
            time_range_text,
            model.guild_name(),
            first_label,
            first_value,
            second_label,
            second_value
        )
    }
}

/// Generates the stats image: the line chart for non-yearly ranges, or the
/// contribution grid for the yearly range.
pub(crate) fn generate_image(
    time_range: VoiceStatsTimeRange,
    stat_type: GuildStatType,
    is_user: bool,
    raw_sessions: &[VoiceSessionsEntity],
    user_activity: &[VoiceDailyActivity],
    guild_stats: &[GuildDailyStats],
) -> anyhow::Result<Vec<u8>> {
    if time_range != VoiceStatsTimeRange::Yearly {
        return generate_line_chart(raw_sessions, time_range, stat_type, is_user);
    }

    let (since, _until) = time_range.to_range();
    let today = chrono::Local::now().date_naive();

    let mut data_map: HashMap<NaiveDate, u32> = HashMap::new();
    if is_user {
        for activity in user_activity {
            let minutes = (activity.total_seconds / 60).max(1) as u32;
            data_map.insert(activity.day, minutes);
        }
    } else {
        for stat in guild_stats {
            let value = if stat_type == GuildStatType::AverageTime
                || stat_type == GuildStatType::TotalTime
            {
                (stat.value / 60).max(1) as u32
            } else {
                stat.value as u32
            };
            data_map.insert(stat.day, value);
        }
    }

    let img = ContributionGraph::new()
        .with_data(data_map)
        .start_date(since.date_naive())
        .end_date(today)
        .theme(Theme::github(Strategy::linear()))
        .generate();

    let mut bytes: Vec<u8> = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )?;

    Ok(bytes)
}

/// The effect adapter that fetches stats data and renders the chart image.
pub struct VoiceStatsEffectHandler {
    service: Arc<dyn VoiceTracker>,
    guild_id: u64,
    guild_name: String,
    http: Arc<Http>,
}

impl VoiceStatsEffectHandler {
    /// Creates an adapter bound to the guild's voice tracking service.
    pub fn new(
        service: Arc<dyn VoiceTracker>,
        guild_id: u64,
        guild_name: String,
        http: Arc<Http>,
    ) -> Self {
        Self {
            service,
            guild_id,
            guild_name,
            http,
        }
    }
}

impl EffectHandler for VoiceStatsEffectHandler {
    type Effect = VoiceStatsEffect;
    type Msg = VoiceStatsMsg;

    fn execute(
        &mut self,
        effect: VoiceStatsEffect,
        tx: tokio::sync::mpsc::UnboundedSender<VoiceStatsMsg>,
    ) -> Vec<VoiceStatsMsg> {
        match effect {
            VoiceStatsEffect::QueryStats {
                time_range,
                stat_type,
                user_id,
            } => {
                let service = self.service.clone();
                let guild_id = self.guild_id;
                let guild_name = self.guild_name.clone();
                let http = self.http.clone();
                tokio::spawn(async move {
                    let data = fetch_stats(
                        service, guild_id, guild_name, http, time_range, stat_type, user_id,
                    )
                    .await;
                    let _ = tx.send(VoiceStatsMsg::StatsLoaded(data));
                });
                vec![]
            }
            VoiceStatsEffect::RenderImage {
                time_range,
                stat_type,
                is_user,
                raw_sessions,
                user_activity,
                guild_stats,
            } => {
                let bytes = generate_image(
                    time_range,
                    stat_type,
                    is_user,
                    &raw_sessions,
                    &user_activity,
                    &guild_stats,
                )
                .ok();
                vec![VoiceStatsMsg::ImageRendered(bytes)]
            }
        }
    }
}

/// Fetches a stats snapshot for the given parameters.
async fn fetch_stats(
    service: Arc<dyn VoiceTracker>,
    guild_id: u64,
    guild_name: String,
    http: Arc<Http>,
    time_range: VoiceStatsTimeRange,
    stat_type: GuildStatType,
    user_id: Option<u64>,
) -> VoiceStatsData {
    let (since, until) = time_range.to_range();

    let raw_sessions = if time_range != VoiceStatsTimeRange::Yearly {
        service
            .get_sessions_in_range(guild_id, user_id, &since, &until)
            .await
            .unwrap_or_else(|e| {
                log::error!("voice stats sessions fetch failed: {e}");
                vec![]
            })
    } else {
        vec![]
    };

    let target_user_name = if let Some(uid) = user_id {
        UserId::new(uid)
            .to_user(&http)
            .await
            .ok()
            .map(|u| u.name.to_string())
    } else {
        None
    };

    if let Some(uid) = user_id {
        let user_activity = service
            .get_user_daily_activity(uid, guild_id, &since, &until)
            .await
            .unwrap_or_else(|e| {
                log::error!("voice stats user activity fetch failed: {e}");
                vec![]
            });

        VoiceStatsData {
            guild_name,
            user_activity,
            guild_stats: vec![],
            raw_sessions,
            target_user_name,
        }
    } else {
        let guild_stats = service
            .get_guild_daily_stats(guild_id, &since, &until, stat_type)
            .await
            .unwrap_or_else(|e| {
                log::error!("voice stats guild stats fetch failed: {e}");
                vec![]
            });

        VoiceStatsData {
            guild_name,
            user_activity: vec![],
            guild_stats,
            raw_sessions,
            target_user_name: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::bot::gui::cycle;

    #[test]
    fn voice_stats_render_snapshot_guild_state() {
        let model = VoiceStatsModel::new(
            VoiceStatsTimeRange::Monthly,
            GuildStatType::TotalTime,
            None,
            1,
            VoiceStatsData {
                guild_name: "Test Server".to_string(),
                user_activity: vec![],
                guild_stats: vec![
                    GuildDailyStats {
                        day: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                        value: 3600,
                    },
                    GuildDailyStats {
                        day: NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
                        value: 7200,
                    },
                ],
                raw_sessions: vec![],
                target_user_name: None,
            },
            None,
        );
        let value = cycle::capture::<VoiceStatsFeature>(&model);
        assert_eq!(
            value,
            json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "### Voice Stats\n-# Time Range: **Monthly** — <t:TS:f> to <t:TS:R>\n\n**Server:** Test Server\n**Peak Total Time:** 2h\n**Most Active:** 2h on <t:TS:d>"
                        },
                        { "type": 14, "divider": true },
                        {
                            "type": 12,
                            "items": [ { "media": { "url": "attachment://voice_stats.png" } } ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:VoiceStatsAction",
                                    "disabled": false,
                                    "label": "Show user stats",
                                    "style": 1
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Yearly", "style": 2 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Monthly", "style": 1 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Weekly", "style": 2 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Hourly", "style": 2 }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Unique Users", "style": 2 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Total Time", "style": 1 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Average Time", "style": 2 }
                    ]
                }
            ])
        );
    }

    #[test]
    fn voice_stats_render_snapshot_user_state() {
        let model = VoiceStatsModel::new(
            VoiceStatsTimeRange::Monthly,
            GuildStatType::TotalTime,
            Some(123_456_789),
            1,
            VoiceStatsData {
                guild_name: "Test Server".to_string(),
                user_activity: vec![],
                guild_stats: vec![],
                raw_sessions: vec![],
                target_user_name: Some("tester".to_string()),
            },
            None,
        );
        let value = cycle::capture::<VoiceStatsFeature>(&model);
        assert_eq!(
            value,
            json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "### Voice Stats\n-# Time Range: **Monthly** — <t:TS:f> to <t:TS:R>\n\n**User:** tester\n**Total Time:** 0s\n**Average Daily:** 0s\n**Current Streak:** 0 day(s)"
                        },
                        { "type": 14, "divider": true },
                        {
                            "type": 10,
                            "content": "No voice activity recorded for this time range.\n\nJoin a **voice channel** to start tracking!"
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:VoiceStatsAction",
                                    "disabled": false,
                                    "label": "Show server stats",
                                    "style": 1
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Yearly", "style": 2 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Monthly", "style": 1 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Weekly", "style": 2 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Hourly", "style": 2 }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Total Time", "style": 1 },
                        { "type": 2, "custom_id": "id:VoiceStatsAction", "disabled": false, "label": "Average Time", "style": 2 }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 5,
                            "custom_id": "id:VoiceStatsAction",
                            "default_values": [ { "id": 123456789, "type": "user" } ]
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn toggle_data_mode_button_switches_to_user_stats_and_requests_a_query() {
        let mut model = VoiceStatsModel::new(
            VoiceStatsTimeRange::Monthly,
            GuildStatType::AverageTime,
            None,
            1,
            VoiceStatsData {
                guild_name: "Test Server".to_string(),
                user_activity: vec![],
                guild_stats: vec![],
                raw_sessions: vec![],
                target_user_name: None,
            },
            None,
        );
        assert!(!model.is_user_stats());

        let registry = cycle::view_actions::<VoiceStatsFeature>(&model);
        assert!(!cycle::has_label(&registry, "SelectUser"));
        let toggle = cycle::find_by_rendered_label::<VoiceStatsFeature>(&model, "Show user stats");
        let msg = cycle::translate_action::<VoiceStatsFeature>(&toggle, &model);
        assert_eq!(msg, VoiceStatsMsg::ToggleDataMode);

        let effects = VoiceStatsFeature::update(msg, &mut model);
        assert!(model.is_user_stats());
        assert_eq!(
            effects,
            vec![VoiceStatsEffect::QueryStats {
                time_range: VoiceStatsTimeRange::Monthly,
                stat_type: GuildStatType::AverageTime,
                user_id: Some(1),
            }]
        );

        // The re-rendered view reflects user stats: the toggle label flips,
        // the unique-users button disappears, and the user-select row is
        // registered.
        let after = cycle::capture::<VoiceStatsFeature>(&model);
        let after_json = serde_json::to_string(&after).unwrap();
        assert!(after_json.contains("Show server stats"));
        assert!(!after_json.contains("Unique Users"));
        let after_registry = cycle::view_actions::<VoiceStatsFeature>(&model);
        assert!(cycle::has_label(&after_registry, "SelectUser"));
    }
}
