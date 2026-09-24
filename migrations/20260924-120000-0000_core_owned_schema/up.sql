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

CREATE TABLE IF NOT EXISTS plugin_kv (
    namespace TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (namespace, key)
);

CREATE TABLE IF NOT EXISTS guild_plugins (
    guild_id BIGINT NOT NULL,
    plugin_name TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (guild_id, plugin_name)
);
