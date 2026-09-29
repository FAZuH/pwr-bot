-- Each migration source owns only the tables in its up.sql and uses CREATE TABLE IF NOT EXISTS.
CREATE TABLE IF NOT EXISTS welcome_settings (
    guild_id BIGINT PRIMARY KEY,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    channel_id TEXT,
    primary_color TEXT,
    template_id TEXT,
    messages JSONB
);
