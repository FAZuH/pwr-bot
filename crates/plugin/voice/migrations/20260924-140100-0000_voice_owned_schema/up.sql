-- Each migration source owns only the tables in its up.sql and uses CREATE TABLE IF NOT EXISTS.
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

CREATE TABLE IF NOT EXISTS voice_settings (
    guild_id BIGINT PRIMARY KEY,
    enabled BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE TABLE IF NOT EXISTS voice_settings_import_state (
    id SMALLINT PRIMARY KEY CHECK (id = 1)
);
