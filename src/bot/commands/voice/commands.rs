use std::time::Duration;

use poise::CreateReply;
use serenity::all::ComponentInteractionCollector;
use serenity::all::CreateAttachment;
use serenity::all::CreateInteractionResponse;
use serenity::all::CreateInteractionResponseMessage;
use serenity::all::MessageFlags;
use serenity::futures::StreamExt;

use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::voice::LeaderboardEntry;
use crate::bot::commands::voice::image_generator::LeaderboardImageGenerator;
use crate::bot::commands::voice::views::LeaderboardView;
use crate::bot::commands::voice::views::SettingsVoiceView;
use crate::bot::error::BotError;
use crate::bot::views::InteractableComponentView;
use crate::bot::views::pagination::PaginationView;
use crate::database::model::VoiceLeaderboardEntry;

const INTERACTION_TIMEOUT_SECS: u64 = 120;
const LEADERBOARD_PER_PAGE: u32 = 10;

pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
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

pub async fn leaderboard(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
    let author_id = ctx.author().id.get();

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
    let mut pagination = PaginationView::new(total_items, LEADERBOARD_PER_PAGE);

    let user_rank = total_entries
        .iter()
        .position(|e| e.user_id == author_id)
        .map(|pos| pos as u32 + 1);

    let image_gen = LeaderboardImageGenerator::new().map_err(|e| {
        Error::from(std::io::Error::other(format!(
            "Failed to initialize image generator: {}",
            e
        )))
    })?;

    let current_page_entries =
        &total_entries[..(total_entries.len().min(LEADERBOARD_PER_PAGE as usize))];
    let page_result = generate_page(ctx, &image_gen, current_page_entries, 0).await?;

    let view = LeaderboardView {};
    let mut components = view.create_page(user_rank);
    pagination.attach_if_multipage(&mut components);
    let attachment = CreateAttachment::bytes(page_result.image_bytes, "leaderboard.png");

    let reply = CreateReply::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(components)
        .attachment(attachment);

    let msg_handle = ctx.send(reply).await?;

    while pagination
        .listen_once(&ctx, Duration::from_secs(INTERACTION_TIMEOUT_SECS))
        .await
        .is_some()
    {
        let current_page = pagination.state.current_page;
        let offset = ((current_page - 1) * LEADERBOARD_PER_PAGE) as usize;
        let end = (offset + LEADERBOARD_PER_PAGE as usize).min(total_entries.len());

        let page_entries = &total_entries[offset..end];
        let page_result = generate_page(ctx, &image_gen, page_entries, offset as u32).await?;

        components = view.create_page(user_rank);
        pagination.attach_if_multipage(&mut components);

        let attachment = CreateAttachment::bytes(page_result.image_bytes, "leaderboard.png");

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
) -> Result<super::PageGenerationResult, Error> {
    let mut entries_with_names: Vec<(VoiceLeaderboardEntry, String)> = Vec::new();
    let mut entries_for_image: Vec<LeaderboardEntry> = Vec::new();

    for (idx, entry) in entries.iter().enumerate() {
        let rank = rank_offset + idx as u32 + 1;

        let (display_name, avatar_url) = match ctx.http().get_user(entry.user_id.into()).await {
            Ok(user) => {
                let name = user.name.to_string();
                let avatar = user.avatar_url();
                (name, avatar)
            }
            Err(_) => (format!("User {}", entry.user_id), None),
        };

        entries_with_names.push((entry.clone(), display_name.clone()));
        entries_for_image.push(LeaderboardEntry {
            rank,
            user_id: entry.user_id,
            display_name,
            avatar_url,
            duration_seconds: entry.total_duration,
        });
    }

    let image_bytes = image_gen
        .generate_leaderboard(&entries_for_image)
        .await
        .map_err(|e| {
            Error::from(std::io::Error::other(format!(
                "Failed to generate leaderboard image: {}",
                e
            )))
        })?;

    Ok(super::PageGenerationResult {
        entries_with_names,
        image_bytes,
    })
}
