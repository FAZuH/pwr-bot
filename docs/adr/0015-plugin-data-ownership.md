# Plugin data ownership

Plugins choose how they store their data; the host provides the
capabilities, not the schemas. The core Postgres connection capability
(`db_url`, already returned by `host.get_config`) is formalized as stable
protocol surface, and `data_path` is the sanctioned home for plugin files
(the voice session heartbeat is a file there, not a table). A plugin owns its
tables and can run its own embedded Diesel migrations at startup. Feed and
voice own their embedded migrations; core owns shared host tables.
The shared `server_settings` table remains for Welcome settings and one-time
legacy imports. Core also keeps `plugin_kv`, `guild_plugins`, and `bot_meta`;
the feed repository is read-only for the transitional dump. Rejected: a generic SQL passthrough cap (violates ADR-0010's "ops
are shaped by services") and typed-RPC growth (keeps the core domain-aware,
contra ADR-0014). No data migration guards the rename of the settings
panels — nothing pre-plugin runs in production, so `guild_plugins` rows are
renamed freely.

**Migration invariant:** Every migration version is globally unique and owned by
one component, and each embed contains only its owner's versions. Diesel 2.x
stores only `version` and `run_on` in `__diesel_schema_migrations`; pending
migration ignores ledger rows whose versions are absent from the embeds. Every
`up.sql` uses `CREATE TABLE IF NOT EXISTS` and creates only the owner's tables;
every `down.sql` drops only those tables. Adoption migrations are idempotent, so
pre-existing tables and data are never touched.

Core uses `20260925-000000-0000_core_storage` for `server_settings`, `bot_meta`,
`plugin_kv`, and `guild_plugins`. Feed uses
`20260924-130100-0000_feed_owned_schema`; voice uses
`20260924-140100-0000_voice_owned_schema`. Feed copies a legacy
`server_settings.feeds` value once into its own storage, then reads only that
storage. Voice imports the legacy `server_settings.voice` value once. The
heartbeat marker is `$DATA_PATH/voice_heartbeat`; it is not a host table or a
plugin-kv row.
