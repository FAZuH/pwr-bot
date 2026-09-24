CREATE TABLE IF NOT EXISTS feed_settings (
    guild_id BIGINT PRIMARY KEY,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    channel_id TEXT,
    subscribe_role_id TEXT,
    unsubscribe_role_id TEXT
);
