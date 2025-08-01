-- Add down migration script here

PRAGMA foreign_keys = OFF;

-- Recreate original tables
CREATE TABLE latest_updates_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    `type` TEXT NOT NULL,
    series_id TEXT NOT NULL,
    series_latest TEXT NOT NULL,
    series_title TEXT NOT NULL,
    series_published TIMESTAMP NOT NULL
);
    -- UNIQUE(`type`, series_id)

CREATE TABLE subscribers_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subscriber_type TEXT NOT NULL,
    subscriber_id TEXT NOT NULL,
    latest_update_id INTEGER,
    UNIQUE(subscriber_type, subscriber_id, latest_update_id),
    FOREIGN KEY (latest_update_id) REFERENCES latest_updates_new(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE
);

-- Migrate data back (this is lossy - we can't perfectly reconstruct type/series_id)
INSERT INTO latest_updates_new (id, `type`, series_id, series_latest, series_title, series_published)
SELECT 
    id,
    CASE 
        WHEN url LIKE "%mangadex.org%" THEN "manga"
        WHEN url LIKE "%anilist.co%" THEN "anime"
        ELSE "unknown"
    END as `type`,
    -- Extract ID from URL (this is approximate)
    CASE 
        WHEN url LIKE "https://mangadex.org/title/%" THEN 
            substr(url, 29, 65)
        WHEN url LIKE 'https://anilist.co/anime/%' THEN 
            substr(url, 26, 
                COALESCE(
                    NULLIF(instr(substr(url, 26), '/') - 1, -1),
                    length(substr(url, 26))
                )
            )
        ELSE url
    END as series_id,
    latest as series_latest,
    name as series_title,
    published as series_published
FROM latest_results;

INSERT INTO subscribers_new SELECT id, subscriber_type, subscriber_id, latest_results_id FROM subscribers;

DROP TABLE subscribers;
DROP TABLE latest_results;

ALTER TABLE latest_updates_new RENAME TO latest_updates;
ALTER TABLE subscribers_new RENAME TO subscribers;

PRAGMA foreign_keys = ON;
