use std::time::Duration;

use poise::CreateReply;
use serenity::all::{CreateAttachment, CreateInteractionResponse, CreateInteractionResponseMessage, MessageFlags};
use serenity::all::ComponentInteractionCollector;
use serenity::futures::StreamExt;

use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::voice::image_generator::LeaderboardImageGenerator;
use crate::bot::commands::voice::view::{LeaderboardView, SettingsVoiceView};
use crate::bot::error::BotError;
use crate::bot::views::{PageNavigationView, Pagination};
use crate::database::model::VoiceLeaderboardEntry;

const INTERACTION_TIMEOUT_SECS: u64 = 120;
const LEADERBOARD_PER_PAGE: u32 = 10;

pub struct SettingsController;

impl SettingsController {
    pub async fn execute(ctx: Context<'_>) -> Result<(), Error> {
        ctx.defer().await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let mut settings = ctx
            .data()
            .service
            .voice_tracking
            .get_server_settings(guild_id)
            .await
            .map_err(Error::from)?;

        let msg_handle = ctx.send(SettingsVoiceView::create_reply(&settings)).await?;

        let msg = msg_handle.message().await?.into_owned();
        let author_id = ctx.author().id;

        let mut collector = ComponentInteractionCollector::new(ctx.serenity_context())
            .message_id(msg.id)
            .author_id(author_id)
            .timeout(Duration::from_secs(INTERACTION_TIMEOUT_SECS))
            .stream();

        while let Some(interaction) = collector.next().await {
            let mut should_update = true;

            match &interaction.data.kind {
                serenity::all::ComponentInteractionDataKind::StringSelect { values }
                    if interaction.data.custom_id == "voice_settings_enabled" =>
                {
                    if let Some(value) = values.first() {
                        settings.voice_tracking_enabled = Some(value == "true");
                    }
                }
                _ => {
                    should_update = false;
                }
            }

            if should_update {
                ctx.data()
                    .service
                    .voice_tracking
                    .update_server_settings(guild_id, settings.clone())
                    .await
                    .map_err(Error::from)?;
            }

            interaction
                .create_response(
                    ctx.http(),
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .components(SettingsVoiceView::create_components(&settings)),
                    ),
                )
                .await?;
        }

        Ok(())
    }
}

pub struct LeaderboardController;

impl LeaderboardController {
    pub async fn execute(ctx: Context<'_>) -> Result<(), Error> {
        ctx.defer().await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
        let author_id = ctx.author().id.get();

        // Get total count for pagination
        let total_entries = ctx
            .data()
            .service
            .voice_tracking
            .get_leaderboard(guild_id, u32::MAX)
            .await
            .map_err(Error::from)?;

        if total_entries.is_empty() {
            let reply = LeaderboardView::create_empty_reply();
            ctx.send(reply).await?;
            return Ok(());
        }

        let total_items = total_entries.len() as u32;
        let pages = total_items.div_ceil(LEADERBOARD_PER_PAGE);
        let pagination = Pagination::new(pages, LEADERBOARD_PER_PAGE, 1);
        let view = PageNavigationView::new(&ctx, pagination);

        // Get user rank
        let user_rank = total_entries
            .iter()
            .position(|e| e.user_id == author_id)
            .map(|pos| pos as u32 + 1);

        // Initialize image generator
        let image_gen = LeaderboardImageGenerator::new().map_err(|e| {
            Error::from(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to initialize image generator: {}", e),
            ))
        })?;

        // Generate initial page
        let current_page_entries = &total_entries[..(total_entries.len().min(LEADERBOARD_PER_PAGE as usize))];
        let (entries_with_names, image_bytes) = Self::generate_page(
            ctx,
            &image_gen,
            current_page_entries,
            0,
        ).await?;

        let leaderboard_view = LeaderboardView::new(view);
        let components = leaderboard_view.create_page(&entries_with_names, user_rank, vec![]);
        let attachment = CreateAttachment::bytes(image_bytes, "leaderboard.png");

        let reply = CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(components)
            .attachment(attachment);

        let msg_handle = ctx.send(reply).await?;

        // Handle pagination
        let mut current_view = leaderboard_view;
        while current_view.navigation().listen(Duration::from_secs(INTERACTION_TIMEOUT_SECS)).await {
            let current_page = current_view.navigation().pagination.current_page;
            let offset = ((current_page - 1) * LEADERBOARD_PER_PAGE) as usize;
            let end = (offset + LEADERBOARD_PER_PAGE as usize).min(total_entries.len());

            let page_entries = &total_entries[offset..end];
            let (entries_with_names, image_bytes) = Self::generate_page(
                ctx,
                &image_gen,
                page_entries,
                offset as u32,
            ).await?;

            let components = current_view.create_page(&entries_with_names, user_rank, vec![]);
            let attachment = CreateAttachment::bytes(image_bytes, "leaderboard.png");

            let reply = CreateReply::new()
                .flags(MessageFlags::IS_COMPONENTS_V2)
                .components(components)
                .attachment(attachment);

            msg_handle.edit(ctx, reply).await?;
        }

        Ok(())
    }

    async fn generate_page(
        ctx: Context<'_>,
        image_gen: &LeaderboardImageGenerator,
        entries: &[VoiceLeaderboardEntry],
        rank_offset: u32,
    ) -> Result<(Vec<(VoiceLeaderboardEntry, String)>, Vec<u8>), Error> {
        // Fetch display names and avatars for all users
        let mut entries_with_names: Vec<(VoiceLeaderboardEntry, String)> = Vec::new();
        let mut entries_for_image: Vec<(u32, u64, String, Option<String>, i64)> = Vec::new();

        for (idx, entry) in entries.iter().enumerate() {
            let rank = rank_offset + idx as u32 + 1;
            
            // Fetch user from Discord
            let (display_name, avatar_url) = match ctx.http().get_user(entry.user_id.into()).await {
                Ok(user) => {
                    let name = user.name.to_string();
                    let avatar = user.avatar_url();
                    (name, avatar)
                }
                Err(_) => (format!("User {}", entry.user_id), None),
            };

            entries_with_names.push((entry.clone(), display_name.clone()));
            entries_for_image.push((rank, entry.user_id, display_name, avatar_url, entry.total_duration));
        }

        // Generate leaderboard image
        let image_bytes = image_gen
            .generate_leaderboard(&entries_for_image)
            .await
            .map_err(|e| {
                Error::from(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to generate leaderboard image: {}", e),
                ))
            })?;

        Ok((entries_with_names, image_bytes))
    }
}
