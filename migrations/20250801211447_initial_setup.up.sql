-- Add migration script here

CREATE TABLE IF NOT EXISTS latest_updates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    `type` TEXT NOT NULL,
    series_id TEXT NOT NULL,
    series_latest TEXT NOT NULL,
    series_title TEXT NOT NULL,
    series_published TIMESTAMP NOT NULL,
    UNIQUE(type, series_id)
);


CREATE TABLE IF NOT EXISTS subscribers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subscriber_type TEXT NOT NULL,
    subscriber_id TEXT NOT NULL,
    latest_update_id INTEGER,
    UNIQUE(subscriber_type, subscriber_id, latest_update_id),
    FOREIGN KEY (latest_update_id) REFERENCES latest_updates(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE
);

PRAGMA foreign_keys = ON;
