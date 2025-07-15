#[derive(Clone)]
pub struct NewEpisodeEvent {
    pub series_id: String,
    pub series_type: String,
    pub title: String,
    pub chapter: String,
    pub chapter_id: String,
    pub url: String,
}
