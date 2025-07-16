use std::str::FromStr;

use super::ani_list_source::AniListSource;
use super::manga_dex_source::MangaDexSource;

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Source {
    Anime(AniListSource),
    Manga(MangaDexSource)
}

impl FromStr for Source {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().trim() {
            "anime" => Ok(Source::Anime(AniListSource::new(s.to_string()))),
            "manga" => Ok(Source::Manga(MangaDexSource::new(s.to_string()))),
            _ => Err("Invalid source".to_string()),
        }
    }
}