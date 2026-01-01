-- Add up migration script here
ALTER TABLE feeds ADD COLUMN platform_id TEXT NOT NULL;
ALTER TABLE feeds ADD COLUMN source_id TEXT NOT NULL;
ALTER TABLE feeds ADD COLUMN item_ids TEXT NOT NULL;

-- Up until this point, we only support feeds platforms MangaDex and AniList.
-- We can transform `feeds.url` to `feeds.source_id` and `feeds.item_id` simply by:
--
-- * MangaDex: "https://mangadex.org/title/{uuid}" -> "{id}"
-- * AniList: "https://anilist.co/anime/{id}" -> "{id}"

-- Convert MangaDex URLs: https://mangadex.org/title/{uuid} -> source_id={uuid}, item_id={uuid}, platform_id='MangaDex'
UPDATE feeds
SET platform_id = 'MangaDex',
    source_id = SUBSTR(url, LENGTH('https://mangadex.org/title/') + 1),
    items_id = SUBSTR(url, LENGTH('https://mangadex.org/title/') + 1)
WHERE url LIKE 'https://mangadex.org/title/%';

-- Convert AniList URLs: https://anilist.co/anime/{id} -> source_id={id}, item_id={id}, platform_id='AniList Anime'
UPDATE feeds
SET platform_id = 'AniList Anime',
    source_id = SUBSTR(url, LENGTH('https://anilist.co/anime/') + 1),
    items_id = SUBSTR(url, LENGTH('https://anilist.co/anime/') + 1)
WHERE url LIKE 'https://anilist.co/anime/%';

CREATE UNIQUE INDEX idx_feeds_platform_source ON feeds(platform_id, source_id);

ALTER TABLE feeds RENAME COLUMN url TO source_url;
