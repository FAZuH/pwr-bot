pub mod anilist;
pub mod comick;
pub mod mangadex;
pub mod platforms;
pub mod traits;

pub use anilist::AniListPlatform;
pub use comick::ComickPlatform;
pub use mangadex::MangaDexPlatform;
pub use platforms::Platforms;
pub use traits::*;
