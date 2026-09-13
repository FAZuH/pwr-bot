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