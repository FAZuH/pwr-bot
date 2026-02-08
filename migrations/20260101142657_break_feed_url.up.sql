-- Add up migration script here

PRAGMA foreign_keys = OFF;

-- Backup dependent tables
CREATE TEMPORARY TABLE feed_items_backup AS SELECT * FROM feed_items;
CREATE TEMPORARY TABLE feed_subscriptions_backup AS SELECT * FROM feed_subscriptions;

-- Drop dependent tables
DROP TABLE feed_items;
DROP TABLE feed_subscriptions;

-- Create new feeds table with new schema
CREATE TABLE feeds_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT DEFAULT NULL,
    platform_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    items_id TEXT NOT NULL,
    source_url TEXT NOT NULL,
    cover_url TEXT DEFAULT NULL,
    tags TEXT DEFAULT NULL
);

-- Copy and transform data
INSERT INTO feeds_new (id, name, description, cover_url, tags, platform_id, source_id, items_id, source_url)
SELECT 
    id,
    name,
    description,
    cover_url,
    tags,
    CASE
        WHEN url LIKE 'https://mangadex.org/title/%' THEN 'MangaDex'
        WHEN url LIKE 'https://anilist.co/anime/%' THEN 'AniList Anime'
    END,
    CASE
        WHEN url LIKE 'https://mangadex.org/title/%' THEN SUBSTR(url, LENGTH('https://mangadex.org/title/') + 1)
        WHEN url LIKE 'https://anilist.co/anime/%' THEN SUBSTR(url, LENGTH('https://anilist.co/anime/') + 1)
    END,
    CASE
        WHEN url LIKE 'https://mangadex.org/title/%' THEN SUBSTR(url, LENGTH('https://mangadex.org/title/') + 1)
        WHEN url LIKE 'https://anilist.co/anime/%' THEN SUBSTR(url, LENGTH('https://anilist.co/anime/') + 1)
    END,
    url
FROM feeds;

DROP TABLE feeds;
ALTER TABLE feeds_new RENAME TO feeds;

CREATE UNIQUE INDEX idx_feeds_platform_source ON feeds(platform_id, source_id);
CREATE UNIQUE INDEX idx_feeds_source_url ON feeds(source_url);

-- Recreate dependent tables
CREATE TABLE feed_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    feed_id INTEGER NOT NULL,
    description TEXT NOT NULL,
    published TIMESTAMP NOT NULL,
    UNIQUE(feed_id, published),
    FOREIGN KEY (feed_id) REFERENCES feeds(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE
);

CREATE TABLE feed_subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    feed_id INTEGER NOT NULL,
    subscriber_id INTEGER NOT NULL,
    UNIQUE(feed_id, subscriber_id),
    FOREIGN KEY (feed_id) REFERENCES feeds(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE,
    FOREIGN KEY (subscriber_id) REFERENCES subscribers(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE
);

-- Restore data
INSERT INTO feed_items SELECT * FROM feed_items_backup;
INSERT INTO feed_subscriptions SELECT * FROM feed_subscriptions_backup;

DROP TABLE feed_items_backup;
DROP TABLE feed_subscriptions_backup;

PRAGMA foreign_keys = ON;
