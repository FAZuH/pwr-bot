//! The `/voice leaderboard` feature shell — a [`GuiFeature`] over the pure
//! voice leaderboard core.
//!
//! Renders the ranked title, the author's rank line, the time-range select,
//! the server/partner toggle, the optional partner user-select, and the
//! pagination row. The initial entries and page image arrive at model
//! construction (data-in via `Config`); in-session refetches and page-image
//! renders are [`VoiceLeaderboardEffect`]s executed by the
//! [`VoiceLeaderboardEffectHandler`] adapter.

use std::borrow::Cow;
use std::sync::Arc;

use pwr_ext::component;

use crate::bot::command::prelude::*;
use crate::bot::command::voice::TimeRange;
use crate::bot::command::voice::VoiceLeaderboardTimeRange;
use crate::bot::command::voice::leaderboard::image_builder::LeaderboardImageBuilder;
use crate::bot::gui::effects::EffectHandler;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::feature::sealed;
use crate::bot::view::pagination::PaginationAction;
use crate::bot::view::pagination::PaginationView;
use crate::entity::VoiceLeaderboardEntry;
use crate::entity::VoiceLeaderboardOptBuilder;
use crate::service::traits::VoiceTracker;
use crate::update::voice_leaderboard::LeaderboardData;
use crate::update::voice_leaderboard::VoiceLeaderboardEffect;
use crate::update::voice_leaderboard::VoiceLeaderboardModel;
use crate::update::voice_leaderboard::VoiceLeaderboardMsg;
use crate::update::voice_leaderboard::update as voice_leaderboard_update;

/// Filename for the voice leaderboard image attachment.
pub const IMAGE_FILENAME: &str = "voice_leaderboard.jpg";

/// Number of leaderboard entries per page.
pub const LEADERBOARD_PER_PAGE: u32 = 10;

/// Data-in for the leaderboard feature: the full entry list and its image.
pub struct VoiceLeaderboardConfig {
    pub entries: Vec<VoiceLeaderboardEntry>,
    pub author_id: u64,
    pub per_page: u32,
    pub image_bytes: Option<Vec<u8>>,
}

action_extends! { VoiceLeaderboardAction extends PaginationAction {
    TimeRange,
    ToggleMode,
    SelectUser,
} }

/// The voice leaderboard feature.
pub struct VoiceLeaderboardFeature;

impl sealed::Sealed for VoiceLeaderboardFeature {}

impl GuiFeature for VoiceLeaderboardFeature {
    type Model = VoiceLeaderboardModel;
    type Msg = VoiceLeaderboardMsg;
    type Action = VoiceLeaderboardAction;
    type Effect = VoiceLeaderboardEffect;
    type Config = VoiceLeaderboardConfig;

    fn initial(config: Self::Config) -> Self::Model {
        VoiceLeaderboardModel::from_entries(config.entries, config.author_id, config.per_page)
            .with_image_bytes(config.image_bytes)
    }

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Vec<Self::Effect> {
        voice_leaderboard_update(msg, model)
    }

    fn view<'a>(
        model: &'a Self::Model,
        registry: &mut ActionRegistry<Self::Action>,
    ) -> Vec<CreateComponent<'a>> {
        use VoiceLeaderboardAction::*;
        use VoiceLeaderboardTimeRange::*;

        let title = if model.is_partner_mode() {
            let display_name = model
                .target_user_name()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Your".to_string());
            format!("### {display_name} Voice Partners")
        } else {
            "### Voice Leaderboard".to_string()
        };

        let mut container: Vec<CreateContainerComponent<'_>> =
            vec![CreateContainerComponent::TextDisplay(component! {
                text_display { content: title }
            })];

        if let Some(rank) = model.user_rank() {
            let duration_text = model
                .user_duration()
                .map(format_duration)
                .unwrap_or_else(|| "unknown".to_string());

            container.push(CreateContainerComponent::TextDisplay(component! {
                text_display {
                    content: format!(
                        "\nYou are ranked **#{rank}** on this server with **{duration_text}** of voice activity."
                    )
                }
            }));
        } else if !model.target_is_author() {
            container.push(CreateContainerComponent::TextDisplay(component! {
                text_display { content: "\nYou are not on the leaderboard for this time range." }
            }));
        }

        let (since, until) = model.time_range().to_range();
        container.push(CreateContainerComponent::TextDisplay(component! {
            text_display {
                content: format!(
                    "\n-# Time Range: **{}** — <t:{}:f> to <t:{}:R>",
                    model.time_range().name(),
                    since.timestamp(),
                    until.timestamp(),
                )
            }
        }));

        container.push(CreateContainerComponent::Separator(component! {
            separator { divider: true }
        }));

        if model.is_empty() {
            container.push(CreateContainerComponent::TextDisplay(component! {
                text_display {
                    content: "No voice activity recorded yet at this time range.\n\nJoin a **voice channel** to start tracking!"
                }
            }));
        } else {
            container.push(CreateContainerComponent::MediaGallery(component! {
                media_gallery {
                    media_gallery_item { media: format!("attachment://{IMAGE_FILENAME}") }
                }
            }));
        }

        let toggle_label = if model.is_partner_mode() {
            "Show Server Leaderboard"
        } else {
            "Show Voice Partners"
        };
        let toggle_action = registry.register(ToggleMode);

        container.push(CreateContainerComponent::ActionRow(component! {
            action_row {
                button {
                    custom_id: toggle_action.id,
                    label: toggle_label,
                    style: ButtonStyle::Primary
                }
            }
        }));

        let time_range_action = registry.register(TimeRange);
        let time_range_kind = CreateSelectMenuKind::String {
            options: vec![
                Past24Hours.into(),
                Past72Hours.into(),
                Past7Days.into(),
                Past14Days.into(),
                ThisMonth.into(),
                ThisYear.into(),
                AllTime.into(),
            ]
            .into(),
        };

        let mut components = vec![
            CreateComponent::Container(CreateContainer::new(container)),
            CreateComponent::ActionRow(component! {
                action_row {
                    select_menu {
                        custom_id: time_range_action.id,
                        kind: time_range_kind,
                        placeholder: "Select time range"
                    }
                }
            }),
        ];

        if model.is_partner_mode() {
            let default_users = model
                .target_user_id()
                .map(|id| Cow::Owned(vec![UserId::new(id)]));
            let user_action = registry.register(SelectUser);
            let user_kind = CreateSelectMenuKind::User { default_users };
            components.push(CreateComponent::ActionRow(component! {
                action_row {
                    select_menu {
                        custom_id: user_action.id,
                        kind: user_kind,
                        placeholder: "Select an user to view their voice partners"
                    }
                }
            }));
        }

        let mut pagination =
            PaginationView::new(model.entries().len() as u32, LEADERBOARD_PER_PAGE);
        pagination.state.current_page = model.current_page();
        pagination.disabled = model.pagination_disabled();
        pagination.attach_if_multipage(registry, &mut components, |action| {
            VoiceLeaderboardAction::Base(action)
        });

        components
    }

    fn translate(
        action: &Self::Action,
        values: SelectValues,
        _model: &Self::Model,
    ) -> Option<Self::Msg> {
        match action {
            VoiceLeaderboardAction::Base(inner) => Some(VoiceLeaderboardMsg::Pagination(*inner)),
            VoiceLeaderboardAction::TimeRange => {
                let range = match values {
                    SelectValues::String(v) => v
                        .first()
                        .and_then(|name| VoiceLeaderboardTimeRange::from_display_name(name)),
                    _ => None,
                };
                range.map(VoiceLeaderboardMsg::ChangeTimeRange)
            }
            VoiceLeaderboardAction::ToggleMode => Some(VoiceLeaderboardMsg::ToggleMode),
            VoiceLeaderboardAction::SelectUser => {
                let user_id = match values {
                    SelectValues::User(v) => v.first().map(|id| id.get()),
                    _ => None,
                };
                Some(VoiceLeaderboardMsg::SetTargetUser(user_id))
            }
        }
    }

    fn attachments(model: &Self::Model) -> Vec<CreateAttachment<'static>> {
        model
            .image_bytes()
            .map(|bytes| CreateAttachment::bytes(bytes.to_vec(), IMAGE_FILENAME))
            .into_iter()
            .collect()
    }
}

impl From<VoiceLeaderboardTimeRange> for CreateSelectMenuOption<'static> {
    fn from(range: VoiceLeaderboardTimeRange) -> Self {
        CreateSelectMenuOption::new(range.name(), range.name())
    }
}

/// The effect adapter that fetches leaderboard entries and renders page images.
pub struct VoiceLeaderboardEffectHandler {
    service: Arc<dyn VoiceTracker>,
    guild_id: u64,
    author_id: u64,
    http: Arc<Http>,
    img_builder: Arc<tokio::sync::Mutex<LeaderboardImageBuilder>>,
}

impl VoiceLeaderboardEffectHandler {
    /// Creates an adapter bound to the guild's voice tracking service.
    pub fn new(
        service: Arc<dyn VoiceTracker>,
        guild_id: u64,
        author_id: u64,
        http: Arc<Http>,
    ) -> Self {
        Self {
            service,
            guild_id,
            author_id,
            img_builder: Arc::new(tokio::sync::Mutex::new(LeaderboardImageBuilder::new(
                http.clone(),
            ))),
            http,
        }
    }
}

impl EffectHandler for VoiceLeaderboardEffectHandler {
    type Effect = VoiceLeaderboardEffect;
    type Msg = VoiceLeaderboardMsg;

    fn execute(
        &mut self,
        effect: VoiceLeaderboardEffect,
        tx: tokio::sync::mpsc::UnboundedSender<VoiceLeaderboardMsg>,
    ) -> Vec<VoiceLeaderboardMsg> {
        match effect {
            VoiceLeaderboardEffect::QueryLeaderboard {
                time_range,
                is_partner_mode,
                target_user_id,
            } => {
                let service = self.service.clone();
                let guild_id = self.guild_id;
                let author_id = self.author_id;
                let http = self.http.clone();
                tokio::spawn(async move {
                    let data = fetch_entries(
                        service,
                        guild_id,
                        author_id,
                        http,
                        time_range,
                        is_partner_mode,
                        target_user_id,
                    )
                    .await;
                    let _ = tx.send(VoiceLeaderboardMsg::EntriesLoaded(data));
                });
                vec![]
            }
            VoiceLeaderboardEffect::RenderImage {
                entries,
                rank_offset,
            } => {
                if entries.is_empty() {
                    vec![VoiceLeaderboardMsg::ImageRendered(None)]
                } else {
                    let builder = self.img_builder.clone();
                    tokio::spawn(async move {
                        let mut guard = builder.lock().await;
                        let res = guard.build(&entries, rank_offset).await;
                        let bytes = res.ok().map(|r| r.image_bytes);
                        let _ = tx.send(VoiceLeaderboardMsg::ImageRendered(bytes));
                    });
                    vec![]
                }
            }
        }
    }
}

/// Fetches the full ranked entry list for the given filters.
async fn fetch_entries(
    service: Arc<dyn VoiceTracker>,
    guild_id: u64,
    author_id: u64,
    http: Arc<Http>,
    time_range: VoiceLeaderboardTimeRange,
    is_partner_mode: bool,
    target_user_id: Option<u64>,
) -> LeaderboardData {
    let (since, until) = time_range.to_range();

    let opts = VoiceLeaderboardOptBuilder::default()
        .guild_id(guild_id)
        .limit(Some(u32::MAX))
        .since(Some(since))
        .until(Some(until))
        .build();

    let opts = match opts {
        Ok(opts) => opts,
        Err(e) => {
            log::error!("voice leaderboard options build failed: {e}");
            return LeaderboardData {
                entries: vec![],
                target_user_name: None,
            };
        }
    };

    let entries = if is_partner_mode {
        let target_id = target_user_id.unwrap_or(author_id);
        service
            .get_partner_leaderboard(&opts, target_id)
            .await
            .unwrap_or_else(|e| {
                log::error!("voice leaderboard partner fetch failed: {e}");
                vec![]
            })
    } else {
        service
            .get_leaderboard_withopt(&opts)
            .await
            .unwrap_or_else(|e| {
                log::error!("voice leaderboard fetch failed: {e}");
                vec![]
            })
    };

    let target_user_name = if is_partner_mode {
        if let Some(uid) = target_user_id {
            UserId::new(uid)
                .to_user(&http)
                .await
                .ok()
                .map(|u| u.name.to_string())
        } else {
            None
        }
    } else {
        None
    };

    LeaderboardData {
        entries,
        target_user_name,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Rewrites every `custom_id` of the shape `Type:timestamp:counter` to a
    /// stable sentinel `id:Type`, and redacts now-dependent Discord timestamps
    /// (`<t:digits:f>` → `<t:TS:f>`) from text content, so the rendered shape is
    /// reproducible across runs while still pinning kind/label/style/order.
    fn normalize(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::String(cid)) = map.get("custom_id") {
                    let parts: Vec<&str> = cid.split(':').collect();
                    if parts.len() == 3
                        && parts[1].chars().all(|c| c.is_ascii_digit())
                        && parts[2].chars().all(|c| c.is_ascii_digit())
                    {
                        map.insert(
                            "custom_id".to_string(),
                            serde_json::json!(format!("id:{}", parts[0])),
                        );
                    }
                }
                for v in map.values_mut() {
                    normalize(v);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    normalize(v);
                }
            }
            serde_json::Value::String(s) => {
                let redacted = redact_timestamps(s);
                if redacted != *s {
                    *s = redacted;
                }
            }
            _ => {}
        }
    }

    /// Redacts the numeric unix timestamp in a Discord `<t:...>` tag.
    fn redact_timestamps(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(idx) = rest.find("<t:") {
            let (before, after) = rest.split_at(idx);
            out.push_str(before);
            let close = after.find('>').expect("unclosed <t: tag");
            let (tag, remaining) = after.split_at(close + 1);
            let parts: Vec<&str> = tag.split(':').collect();
            out.push_str("<t:TS");
            for p in &parts[2..] {
                out.push(':');
                out.push_str(p);
            }
            rest = remaining;
        }
        out.push_str(rest);
        out
    }

    fn entry(user_id: u64, duration: i64) -> VoiceLeaderboardEntry {
        VoiceLeaderboardEntry {
            user_id,
            total_duration: duration,
        }
    }

    fn capture(model: &VoiceLeaderboardModel) -> serde_json::Value {
        let mut registry = ActionRegistry::new();
        let components = VoiceLeaderboardFeature::view(model, &mut registry);
        let mut value = serde_json::to_value(&components).unwrap();
        normalize(&mut value);
        value
    }

    #[test]
    fn voice_leaderboard_render_server_snapshot() {
        let entries = vec![entry(100, 3600), entry(200, 7200)];
        let model = VoiceLeaderboardModel::from_entries(entries, 100, LEADERBOARD_PER_PAGE);
        let value = capture(&model);
        assert_eq!(
            value,
            json!([
                {
                    "type": 17,
                    "components": [
                        { "type": 10, "content": "### Voice Leaderboard" },
                        { "type": 10, "content": "\nYou are ranked **#1** on this server with **1h** of voice activity." },
                        { "type": 10, "content": "\n-# Time Range: **This month** — <t:TS:f> to <t:TS:R>" },
                        { "type": 14, "divider": true },
                        {
                            "type": 12,
                            "items": [ { "media": { "url": "attachment://voice_leaderboard.jpg" } } ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:VoiceLeaderboardAction",
                                    "disabled": false,
                                    "label": "Show Voice Partners",
                                    "style": 1
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 3,
                            "custom_id": "id:VoiceLeaderboardAction",
                            "options": [
                                { "label": "Past 24 hours", "value": "Past 24 hours" },
                                { "label": "Past 72 hours", "value": "Past 72 hours" },
                                { "label": "Past 7 days", "value": "Past 7 days" },
                                { "label": "Past 14 days", "value": "Past 14 days" },
                                { "label": "This month", "value": "This month" },
                                { "label": "This year", "value": "This year" },
                                { "label": "All time", "value": "All time" }
                            ],
                            "placeholder": "Select time range"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn voice_leaderboard_render_partner_snapshot() {
        let model = VoiceLeaderboardModel {
            entries: vec![entry(100, 3600), entry(200, 7200)],
            user_rank: None,
            user_duration: None,
            time_range: VoiceLeaderboardTimeRange::ThisMonth,
            is_partner_mode: true,
            target_user_id: Some(200),
            target_user_name: Some("partner".to_string()),
            author_id: 100,
            pagination: crate::update::pagination::PaginationModel {
                current_page: 1,
                pages: 1,
                per_page: LEADERBOARD_PER_PAGE,
            },
            image_bytes: None,
            pagination_disabled: false,
        };
        let value = capture(&model);
        assert_eq!(
            value,
            json!([
                {
                    "type": 17,
                    "components": [
                        { "type": 10, "content": "### partner Voice Partners" },
                        { "type": 10, "content": "\nYou are not on the leaderboard for this time range." },
                        { "type": 10, "content": "\n-# Time Range: **This month** — <t:TS:f> to <t:TS:R>" },
                        { "type": 14, "divider": true },
                        {
                            "type": 12,
                            "items": [ { "media": { "url": "attachment://voice_leaderboard.jpg" } } ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:VoiceLeaderboardAction",
                                    "disabled": false,
                                    "label": "Show Server Leaderboard",
                                    "style": 1
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 3,
                            "custom_id": "id:VoiceLeaderboardAction",
                            "options": [
                                { "label": "Past 24 hours", "value": "Past 24 hours" },
                                { "label": "Past 72 hours", "value": "Past 72 hours" },
                                { "label": "Past 7 days", "value": "Past 7 days" },
                                { "label": "Past 14 days", "value": "Past 14 days" },
                                { "label": "This month", "value": "This month" },
                                { "label": "This year", "value": "This year" },
                                { "label": "All time", "value": "All time" }
                            ],
                            "placeholder": "Select time range"
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 5,
                            "custom_id": "id:VoiceLeaderboardAction",
                            "default_values": [ { "id": 200, "type": "user" } ],
                            "placeholder": "Select an user to view their voice partners"
                        }
                    ]
                }
            ])
        );
    }
}
