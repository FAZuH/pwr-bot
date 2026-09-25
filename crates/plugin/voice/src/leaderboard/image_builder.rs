use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use log::trace;
use pwr_plugin_protocol::ResolvedUser;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;

use crate::VoiceLeaderboardEntry;
use crate::host_client::HostCallError;
use crate::host_client::HostClient;
use crate::leaderboard::image_generator::LeaderboardImageGenerator;

/// One row prepared for image rendering.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub rank: u32,
    pub user_id: u64,
    pub display_name: String,
    pub avatar_url: String,
    pub duration_seconds: i64,
    #[serde(skip)]
    pub avatar_image: Option<image::DynamicImage>,
}

/// The resolved names and rendered bytes for one leaderboard page.
pub struct ImageGenerationResult {
    pub entries_with_names: Vec<(VoiceLeaderboardEntry, String)>,
    pub image_bytes: Vec<u8>,
}

/// Builds a leaderboard image for one guild.
pub struct LeaderboardImageBuilder {
    guild_id: u64,
    host: Arc<dyn HostClient>,
    image_gen: LeaderboardImageGenerator,
    user_cache: HashMap<u64, ResolvedUser>,
}

impl LeaderboardImageBuilder {
    /// Creates a builder that resolves users for the given guild.
    pub fn new(guild_id: u64, host: Arc<dyn HostClient>) -> Self {
        Self {
            guild_id,
            host,
            image_gen: LeaderboardImageGenerator::new(),
            user_cache: HashMap::new(),
        }
    }

    /// Resolves users and renders one page for the supplied entries.
    pub async fn build(
        &mut self,
        entries: &[VoiceLeaderboardEntry],
        rank_offset: u32,
    ) -> Result<ImageGenerationResult, ImageBuildError> {
        let started = Instant::now();
        self.fetch_missing_users(entries).await?;
        let new_avatars = self.fetch_missing_avatars(entries).await;
        trace!(
            "fetch_users_and_avatars_parallel {} ms",
            started.elapsed().as_millis()
        );
        let (entries_with_names, entries_for_image) =
            self.prepare_render_data(entries, rank_offset, &new_avatars);
        let image_bytes = self
            .image_gen
            .generate_leaderboard(&entries_for_image)
            .await
            .map_err(ImageBuildError::Render)?;
        Ok(ImageGenerationResult {
            entries_with_names,
            image_bytes,
        })
    }

    async fn fetch_missing_users(
        &mut self,
        entries: &[VoiceLeaderboardEntry],
    ) -> Result<(), ImageBuildError> {
        let user_ids: Vec<u64> = entries
            .iter()
            .map(|entry| entry.user_id)
            .filter(|user_id| !self.user_cache.contains_key(user_id))
            .collect();
        if user_ids.is_empty() {
            return Ok(());
        }
        let response = self
            .host
            .call(
                "host.resolve_users",
                json!({ "guild_id": self.guild_id, "user_ids": user_ids }),
            )
            .await
            .map_err(ImageBuildError::Host)?;
        let users: Vec<ResolvedUser> = serde_json::from_value(response)
            .map_err(|error| ImageBuildError::Render(anyhow::anyhow!(error)))?;
        for user in users {
            self.user_cache.insert(user.id, user);
        }
        Ok(())
    }

    async fn fetch_missing_avatars(
        &mut self,
        entries: &[VoiceLeaderboardEntry],
    ) -> HashMap<u64, image::DynamicImage> {
        let mut downloads = Vec::new();
        for entry in entries {
            let Some(user) = self.user_cache.get(&entry.user_id) else {
                continue;
            };
            if user.avatar_url.is_empty() || self.image_gen.has_avatar(&user.avatar_url) {
                continue;
            }
            let url = user.avatar_url.clone();
            let client = self.image_gen.http_client.clone();
            downloads.push(async move {
                let image = async move {
                    let response = client.get(&url).send().await.ok()?;
                    let bytes = response.bytes().await.ok()?;
                    image::load_from_memory(&bytes).ok()
                }
                .await;
                (entry.user_id, image)
            });
        }
        futures::future::join_all(downloads)
            .await
            .into_iter()
            .filter_map(|(user_id, image)| image.map(|image| (user_id, image)))
            .collect()
    }

    fn prepare_render_data(
        &self,
        entries: &[VoiceLeaderboardEntry],
        rank_offset: u32,
        new_avatars: &HashMap<u64, image::DynamicImage>,
    ) -> (Vec<(VoiceLeaderboardEntry, String)>, Vec<LeaderboardEntry>) {
        let mut entries_with_names = Vec::new();
        let mut entries_for_image = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let rank = rank_offset + index as u32 + 1;
            let (display_name, avatar_url, avatar_image) = match self.user_cache.get(&entry.user_id)
            {
                Some(user) => (
                    user.name.clone(),
                    user.avatar_url.clone(),
                    new_avatars.get(&entry.user_id).cloned(),
                ),
                None => (format!("User {}", entry.user_id), String::new(), None),
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

#[derive(Debug, thiserror::Error)]
pub enum ImageBuildError {
    #[error("host identity lookup failed: {0}")]
    Host(#[source] HostCallError),
    #[error("leaderboard image rendering failed: {0}")]
    Render(#[source] anyhow::Error),
}
