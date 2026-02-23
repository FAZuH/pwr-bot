use std::collections::HashMap;
use std::time::Instant;

use log::trace;
use poise::serenity_prelude::UserId;

use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::voice::leaderboard::image_generator::LeaderboardImageGenerator;
use crate::entity::VoiceLeaderboardEntry;
use crate::error::AppError;

/// A single entry in the voice leaderboard.
#[derive(Clone)]
pub struct LeaderboardEntry {
    pub rank: u32,
    pub user_id: u64,
    pub display_name: String,
    pub avatar_url: String,
    pub duration_seconds: i64,
    pub avatar_image: Option<image::DynamicImage>,
}

/// Result of generating a leaderboard page.
pub struct PageGenerationResult {
    pub entries_with_names: Vec<(VoiceLeaderboardEntry, String)>,
    pub image_bytes: Vec<u8>,
}

/// Builder for creating leaderboard pages with image generation.
pub struct LeaderboardPageBuilder<'a> {
    ctx: &'a Context<'a>,
    image_gen: LeaderboardImageGenerator,
    user_cache: HashMap<u64, poise::serenity_prelude::User>,
}

impl<'a> LeaderboardPageBuilder<'a> {
    /// Creates a new page builder with initialized image generator.
    pub fn new(ctx: &'a Context<'a>) -> Self {
        let image_gen = LeaderboardImageGenerator::new();
        Self {
            ctx,
            image_gen,
            user_cache: HashMap::new(),
        }
    }

    /// Generates a page for the given entries with the specified rank offset.
    pub async fn build_page(
        &mut self,
        entries: &[VoiceLeaderboardEntry],
        rank_offset: u32,
    ) -> Result<PageGenerationResult, Error> {
        let fetch_start = Instant::now();
        let http_client = self.image_gen.http_client.clone();

        // Fetch missing users
        self.fetch_missing_users(entries).await;

        // Fetch missing avatars
        let new_avatars = self.fetch_missing_avatars(entries, &http_client).await;

        trace!(
            "fetch_users_and_avatars_parallel {} ms",
            fetch_start.elapsed().as_millis()
        );

        // Prepare entries for rendering
        let (entries_with_names, entries_for_image) =
            self.prepare_render_data(entries, rank_offset, &new_avatars);

        // Generate the image
        let init_start = Instant::now();
        let image_bytes = self
            .image_gen
            .generate_leaderboard(&entries_for_image)
            .await
            .map_err(|e| {
                AppError::internal_with_ref(format!("Failed to generate leaderboard image: {}", e))
            })?;
        trace!("generate_took {} ms", init_start.elapsed().as_millis());

        Ok(PageGenerationResult {
            entries_with_names,
            image_bytes,
        })
    }

    /// Fetches user data for entries not in the cache.
    async fn fetch_missing_users(&mut self, entries: &[VoiceLeaderboardEntry]) {
        let missing_users: Vec<_> = entries
            .iter()
            .filter(|e| !self.user_cache.contains_key(&e.user_id))
            .collect();

        if missing_users.is_empty() {
            return;
        }

        let user_futures: Vec<_> = missing_users
            .iter()
            .map(|entry| {
                let user_id = UserId::new(entry.user_id);
                let http = self.ctx.http();
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
            self.user_cache.insert(uid, user);
        }
    }

    /// Fetches avatar images for users not in the cache.
    async fn fetch_missing_avatars(
        &self,
        entries: &[VoiceLeaderboardEntry],
        http_client: &wreq::Client,
    ) -> HashMap<u64, image::DynamicImage> {
        let avatar_futures: Vec<_> = entries
            .iter()
            .filter_map(|entry| {
                let user = self.user_cache.get(&entry.user_id)?;
                let avatar_url = user.static_face();

                if self.image_gen.has_avatar(&avatar_url) {
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
        fetched_avatars
            .into_iter()
            .filter_map(|(uid, img)| img.map(|i| (uid, i)))
            .collect()
    }

    /// Prepares data for rendering by combining cached users with entries.
    fn prepare_render_data(
        &self,
        entries: &[VoiceLeaderboardEntry],
        rank_offset: u32,
        new_avatars: &HashMap<u64, image::DynamicImage>,
    ) -> (Vec<(VoiceLeaderboardEntry, String)>, Vec<LeaderboardEntry>) {
        let mut entries_with_names: Vec<(VoiceLeaderboardEntry, String)> = Vec::new();
        let mut entries_for_image: Vec<LeaderboardEntry> = Vec::new();

        for (idx, entry) in entries.iter().enumerate() {
            let rank = rank_offset + idx as u32 + 1;

            let (display_name, avatar_url, avatar_image) =
                if let Some(user) = self.user_cache.get(&entry.user_id) {
                    let url = user.static_face();
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

        (entries_with_names, entries_for_image)
    }
}
