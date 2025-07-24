use super::anime_update_event::AnimeUpdateEvent;
use super::manga_update_event::MangaUpdateEvent;

pub enum UpdateEvent {
    Manga(MangaUpdateEvent),
    Anime(AnimeUpdateEvent),
}
