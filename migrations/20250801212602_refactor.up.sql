-- SQLite doesn't support FOREIGN_KEY_CHECKS, but we can disable foreign keys
PRAGMA foreign_keys = OFF;

-- 1. Create new table with desired schema
CREATE TABLE latest_results_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    latest TEXT NOT NULL,
    tags TEXT DEFAULT NULL,
    published TIMESTAMP NOT NULL,
    url TEXT NOT NULL,
    UNIQUE(url)
);

-- 2. Migrate data from latest_updates to new table
INSERT INTO latest_results_new (id, name, latest, tags, published, url)
SELECT 
    id,
    series_title as name,
    series_latest as latest,
    "series" as tags,
    series_published as published,
    CASE 
        WHEN lower(`type`) = 'manga' THEN 'https://mangadex.org/title/' || series_id
        WHEN lower(`type`) = 'anime' THEN 'https://anilist.co/anime/' || series_id
        ELSE series_id
    END as url
FROM latest_updates;

-- 3. Create new subscribers table with updated foreign key
CREATE TABLE subscribers_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subscriber_type TEXT NOT NULL,
    subscriber_id TEXT NOT NULL,
    latest_results_id INTEGER,
    UNIQUE(subscriber_type, subscriber_id, latest_results_id),
    FOREIGN KEY (latest_results_id) REFERENCES latest_results_new(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE
);

-- 4. Migrate subscribers data
INSERT INTO subscribers_new (id, subscriber_type, subscriber_id, latest_results_id)
SELECT id, subscriber_type, subscriber_id, latest_update_id
FROM subscribers;

-- 5. Drop old tables
DROP TABLE subscribers;
DROP TABLE latest_updates;

-- 6. Rename new tables
ALTER TABLE latest_results_new RENAME TO latest_results;
ALTER TABLE subscribers_new RENAME TO subscribers;

PRAGMA foreign_keys = ON;
