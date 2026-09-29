-- Each migration source owns only the tables in its up.sql and uses CREATE TABLE IF NOT EXISTS.
CREATE TABLE IF NOT EXISTS server_settings (
    guild_id BIGINT PRIMARY KEY,
    settings JSONB NOT NULL
);

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
