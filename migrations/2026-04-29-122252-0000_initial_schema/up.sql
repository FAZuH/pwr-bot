CREATE TABLE IF NOT EXISTS feeds (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    platform_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    items_id TEXT NOT NULL,
    source_url TEXT NOT NULL,
    cover_url TEXT NOT NULL DEFAULT '',
    tags TEXT NOT NULL DEFAULT ''
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_feeds_platform_source ON feeds(platform_id, source_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_feeds_source_url ON feeds(source_url);

CREATE TABLE IF NOT EXISTS feed_items (
    id SERIAL PRIMARY KEY,
    feed_id INTEGER NOT NULL,
    description TEXT NOT NULL,
    published TIMESTAMPTZ NOT NULL,
    UNIQUE(feed_id, published),
    FOREIGN KEY (feed_id) REFERENCES feeds(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE
);

CREATE TABLE IF NOT EXISTS subscribers (
    id SERIAL PRIMARY KEY,
    type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    UNIQUE(type, target_id)
);

CREATE TABLE IF NOT EXISTS feed_subscriptions (
    id SERIAL PRIMARY KEY,
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

CREATE TABLE IF NOT EXISTS server_settings (
    guild_id BIGINT PRIMARY KEY,
    settings JSONB NOT NULL
);

CREATE TABLE IF NOT EXISTS voice_sessions (
    id SERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    guild_id BIGINT NOT NULL,
    channel_id BIGINT NOT NULL,
    join_time TIMESTAMPTZ NOT NULL,
    leave_time TIMESTAMPTZ NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE(user_id, channel_id, join_time)
);

CREATE INDEX IF NOT EXISTS idx_voice_sessions_partner
ON voice_sessions (guild_id, channel_id, join_time, leave_time);

CREATE TABLE IF NOT EXISTS bot_meta (
    key TEXT NOT NULL PRIMARY KEY,
    value TEXT NOT NULL
);
