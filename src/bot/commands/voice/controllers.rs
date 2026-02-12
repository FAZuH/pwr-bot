//! Voice command implementations.

use std::collections::HashMap;
use std::time::Instant;

use log::trace;
use poise::CreateReply;
use serenity::all::CreateAttachment;
use serenity::all::MessageFlags;
use serenity::all::UserId;

use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::settings::SettingsPage;
use crate::bot::commands::settings::run_settings;
use crate::bot::commands::voice::LeaderboardEntry;
use crate::bot::commands::voice::VoiceLeaderboardTimeRange;
use crate::bot::commands::voice::image_generator::LeaderboardImageGenerator;
use crate::bot::commands::voice::views::SettingsVoiceAction;
use crate::bot::commands::voice::views::SettingsVoiceView;
use crate::bot::commands::voice::views::VoiceLeaderboardView;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::InteractableComponentView;
use crate::bot::views::ResponseComponentView;
use crate::bot::views::pagination::PaginationView;
use crate::controller;
use crate::database::model::VoiceLeaderboardEntry;
use crate::database::model::VoiceLeaderboardOptBuilder;
use crate::error::AppError;

/// Number of leaderboard entries per page.
const LEADERBOARD_PER_PAGE: u32 = 10;

controller! { pub struct VoiceSettingsController<'a> {} }

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for VoiceSettingsController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let settings = ctx
            .data()
            .service
            .voice_tracking
            .get_server_settings(guild_id)
            .await
            .map_err(Error::from)?;

        let mut view = SettingsVoiceView::new(&ctx, settings);
        coordinator.send(view.create_reply()).await?;

        while let Some((action, _interaction)) = view.listen_once().await {
            match action {
                SettingsVoiceAction::Back => return Ok(NavigationResult::Back),
                SettingsVoiceAction::About => {
                    return Ok(NavigationResult::SettingsAbout);
                }
                SettingsVoiceAction::ToggleEnabled => {
                    // Update the settings in the database
                    ctx.data()
                        .service
                        .voice_tracking
                        .update_server_settings(guild_id, view.settings.clone())
                        .await
                        .map_err(Error::from)?;

                    coordinator.edit(view.create_reply()).await?;
                }
            }
        }

        Ok(NavigationResult::Exit)
    }
}

pub struct VoiceLeaderboardController<'a> {
    #[allow(dead_code)]
    ctx: &'a Context<'a>,
    pub time_range: VoiceLeaderboardTimeRange,
    pub user_cache: HashMap<u64, serenity::all::User>,
}

impl<'a> VoiceLeaderboardController<'a> {
    pub fn new(ctx: &'a Context<'a>, time_range: VoiceLeaderboardTimeRange) -> Self {
        Self {
            ctx,
            time_range,
            user_cache: HashMap::new(),
        }
    }
}

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for VoiceLeaderboardController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let controller_start = Instant::now();

        let ctx = *coordinator.context();
        ctx.defer().await?;

        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
        let author_id = ctx.author().id.get();
        let (since, until) = self.time_range.to_range();

        let query_start = Instant::now();
        let voice_lb_otps = VoiceLeaderboardOptBuilder::default()
            .guild_id(guild_id)
            // TODO: Performance should be improved or this will become a problem
            // NOTE: We need this to compute the author's rank
            .limit(Some(u32::MAX))
            .since(Some(since))
            .until(Some(until))
            .build()
            .map_err(AppError::from)?;
        let total_entries = ctx
            .data()
            .service
            .voice_tracking
            .get_leaderboard_withopt(&voice_lb_otps)
            .await
            .map_err(Error::from)?;
        trace!("query_leaderboard {} ms", query_start.elapsed().as_millis());

        if total_entries.is_empty() {
            let reply = VoiceLeaderboardView::create_empty_reply();
            ctx.send(reply).await?;
            return Ok(NavigationResult::Exit);
        }

        let total_items = total_entries.len() as u32;
        let mut pagination = PaginationView::new(&ctx, total_items, LEADERBOARD_PER_PAGE);
        let user_rank = total_entries
            .iter()
            .position(|e| e.user_id == author_id)
            .map(|pos| pos as u32 + 1);

        let init_gen_start = Instant::now();
        let mut image_gen = LeaderboardImageGenerator::new().map_err(|e| {
            AppError::internal_with_ref(format!("Failed to initialize image generator: {e}"))
        })?;
        trace!(
            "init_image_generator {} ms",
            init_gen_start.elapsed().as_millis()
        );

        let first_page_start = Instant::now();
        let current_page_entries =
            &total_entries[..(total_entries.len().min(LEADERBOARD_PER_PAGE as usize))];
        let page_result = generate_page(
            ctx,
            &mut image_gen,
            &mut self.user_cache,
            current_page_entries,
            0,
        )
        .await?;
        trace!(
            "first_page_generation {} ms",
            first_page_start.elapsed().as_millis()
        );

        let view = VoiceLeaderboardView::new(user_rank);
        let mut components = view.create_components();
        pagination.attach_if_multipage(&mut components);

        let send_start = Instant::now();
        let attachment = CreateAttachment::bytes(page_result.image_bytes, "voice_leaderboard.jpg");
        let reply = CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(components)
            .attachment(attachment);
        let msg_handle = ctx.send(reply).await?;
        trace!(
            "send_initial_message {} ms",
            send_start.elapsed().as_millis()
        );
        trace!(
            "controller_initial_response {} ms",
            controller_start.elapsed().as_millis()
        );

        while pagination.listen_once().await.is_some() {
            let page_update_start = Instant::now();

            let current_page = pagination.state.current_page;
            let offset = ((current_page - 1) * LEADERBOARD_PER_PAGE) as usize;
            let end = (offset + LEADERBOARD_PER_PAGE as usize).min(total_entries.len());
            let page_entries = &total_entries[offset..end];

            let page_result = generate_page(
                ctx,
                &mut image_gen,
                &mut self.user_cache,
                page_entries,
                offset as u32,
            )
            .await?;

            components = view.create_components();
            pagination.attach_if_multipage(&mut components);

            let edit_start = Instant::now();
            let attachment =
                CreateAttachment::bytes(page_result.image_bytes, "voice_leaderboard.jpg");
            let reply = CreateReply::new()
                .flags(MessageFlags::IS_COMPONENTS_V2)
                .components(components)
                .attachment(attachment);
            msg_handle.edit(ctx, reply).await?;
            trace!("edit_message {} ms", edit_start.elapsed().as_millis());
            trace!(
                "page_update total {} ms",
                page_update_start.elapsed().as_millis()
            );
        }

        trace!(
            "controller_total {} ms",
            controller_start.elapsed().as_millis()
        );
        Ok(NavigationResult::Exit)
    }
}

/// Legacy function for voice settings command.
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    run_settings(ctx, Some(SettingsPage::Voice)).await
}

/// Legacy function for voice leaderboard command.
pub async fn leaderboard(
    ctx: Context<'_>,
    time_range: VoiceLeaderboardTimeRange,
) -> Result<(), Error> {
    let mut coordinator = Coordinator::new(ctx);
    let mut controller = VoiceLeaderboardController::new(&ctx, time_range);
    let _result = controller.run(&mut coordinator).await?;
    Ok(())
}

/// Generates a single page of the leaderboard.
async fn generate_page(
    ctx: Context<'_>,
    image_gen: &mut LeaderboardImageGenerator,
    user_cache: &mut HashMap<u64, serenity::all::User>,
    entries: &[VoiceLeaderboardEntry],
    rank_offset: u32,
) -> Result<super::PageGenerationResult, Error> {
    let fetch_start = Instant::now();
    let http_client = image_gen.http_client.clone();

    // Identify users and avatars we need to fetch
    // 1. Users not in user_cache
    // 2. Avatars not in image_gen.avatar_cache (we check this inside the parallel loop)

    // Filter entries that need user fetching
    let missing_users: Vec<_> = entries
        .iter()
        .filter(|e| !user_cache.contains_key(&e.user_id))
        .collect();

    if !missing_users.is_empty() {
        let user_futures: Vec<_> = missing_users
            .iter()
            .map(|entry| {
                let user_id = UserId::new(entry.user_id);
                let http = ctx.http();
                async move {
                    user_id
                        .to_user(&http)
                        .await
                        .ok()
                        .map(|u| (entry.user_id, u))
                }
            })
            .collect();

        let fetched_users: Vec<_> = futures::future::join_all(user_futures).await;
        for (uid, user) in fetched_users.into_iter().flatten() {
            user_cache.insert(uid, user);
        }
    }

    // Now check for avatars that need downloading
    // We only need to download if image_gen doesn't have it
    // We need the avatar URL from the user object (now in cache)
    let avatar_futures: Vec<_> = entries
        .iter()
        .filter_map(|entry| {
            let user = user_cache.get(&entry.user_id)?;
            let avatar_url = user.static_face();

            if image_gen.has_avatar(&avatar_url) {
                return None;
            }

            let client = http_client.clone();
            let uid = entry.user_id;

            Some(async move {
                let img = if let Ok(resp) = client.get(&avatar_url).send().await {
                    if let Ok(bytes) = resp.bytes().await {
                        image::load_from_memory(&bytes).ok()
                    } else {
                        None
                    }
                } else {
                    None
                };
                (uid, img)
            })
        })
        .collect();

    let fetched_avatars: Vec<_> = futures::future::join_all(avatar_futures).await;
    let mut new_avatars: HashMap<u64, image::DynamicImage> = HashMap::new();

    for (uid, img) in fetched_avatars {
        if let Some(image) = img {
            new_avatars.insert(uid, image);
        }
    }

    trace!(
        "fetch_users_and_avatars_parallel {} ms",
        fetch_start.elapsed().as_millis()
    );

    let mut entries_with_names: Vec<(VoiceLeaderboardEntry, String)> = Vec::new();
    let mut entries_for_image: Vec<LeaderboardEntry> = Vec::new();

    for (idx, entry) in entries.iter().enumerate() {
        let rank = rank_offset + idx as u32 + 1;

        let (display_name, avatar_url, avatar_image) =
            if let Some(user) = user_cache.get(&entry.user_id) {
                let url = user.static_face();
                // If we just downloaded it, use it. Otherwise None (image_gen will use cache)
                let img = new_avatars.get(&entry.user_id).cloned();
                (user.name.to_string(), url, img)
            } else {
                (format!("User {}", entry.user_id), String::new(), None)
            };

        entries_with_names.push((entry.clone(), display_name.clone()));
        entries_for_image.push(LeaderboardEntry {
            rank,
            user_id: entry.user_id,
            display_name,
            avatar_url,
            duration_seconds: entry.total_duration,
            avatar_image,
        });
    }

    let init_start = Instant::now();
    let image_bytes = image_gen
        .generate_leaderboard(&entries_for_image)
        .await
        .map_err(|e| {
            AppError::internal_with_ref(format!("Failed to initialize image generator: {}", e))
        })?;
    trace!("generate_took {} ms", init_start.elapsed().as_millis());

    Ok(super::PageGenerationResult {
        entries_with_names,
        image_bytes,
    })
}
