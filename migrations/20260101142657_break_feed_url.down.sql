-- Add down migration script here

PRAGMA foreign_keys = OFF;

-- Backup dependent tables
CREATE TEMPORARY TABLE feed_items_backup AS SELECT * FROM feed_items;
CREATE TEMPORARY TABLE feed_subscriptions_backup AS SELECT * FROM feed_subscriptions;

-- Drop dependent tables
DROP TABLE feed_items;
DROP TABLE feed_subscriptions;

-- Create old feeds table
CREATE TABLE feeds_old (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT DEFAULT NULL,
    url TEXT NOT NULL UNIQUE,
    cover_url TEXT DEFAULT NULL,
    tags TEXT DEFAULT NULL
);

-- Restore data with reconstructed URL
INSERT INTO feeds_old (id, name, description, url, cover_url, tags)
SELECT 
    id,
    name,
    description,
    source_url,
    cover_url,
    tags
FROM feeds;

DROP TABLE feeds;
ALTER TABLE feeds_old RENAME TO feeds;

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
