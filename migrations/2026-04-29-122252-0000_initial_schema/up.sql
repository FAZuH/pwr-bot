PRAGMA foreign_keys = ON;

CREATE TABLE feeds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    platform_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    items_id TEXT NOT NULL,
    source_url TEXT NOT NULL,
    cover_url TEXT NOT NULL DEFAULT '',
    tags TEXT NOT NULL DEFAULT ''
);

CREATE UNIQUE INDEX idx_feeds_platform_source ON feeds(platform_id, source_id);
CREATE UNIQUE INDEX idx_feeds_source_url ON feeds(source_url);

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

CREATE TABLE subscribers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    UNIQUE(type, target_id)
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

CREATE TABLE server_settings (
    guild_id BIGINT PRIMARY KEY,
    settings TEXT NOT NULL
);

CREATE TABLE voice_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id BIGINT NOT NULL,
    guild_id BIGINT NOT NULL,
    channel_id BIGINT NOT NULL,
    join_time TIMESTAMP NOT NULL,
    leave_time TIMESTAMP NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 0,
    UNIQUE(user_id, channel_id, join_time)
);

CREATE INDEX idx_voice_sessions_partner
ON voice_sessions (guild_id, channel_id, join_time, leave_time);

CREATE TABLE bot_meta (
    key TEXT NOT NULL PRIMARY KEY,
    value TEXT NOT NULL
);
