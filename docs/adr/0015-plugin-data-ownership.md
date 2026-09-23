# Plugin data ownership

Plugins choose how they store their data; the host provides the
capabilities, not the schemas. The core Postgres connection capability
(`db_url`, already returned by `host.get_config`) is formalized as stable
protocol surface, and `data_path` is the sanctioned home for plugin files
(the voice session heartbeat is a file there, not a table). Each plugin
owns its tables and runs its own embedded Diesel migrations at startup.
The shared `server_settings` table — an artifact of one process owning
three domains — splits into per-plugin settings storage, with each plugin
reading its legacy row on first startup; core keeps only `plugin_kv`,
`guild_plugins`, and `bot_meta`, and drops `SettingsService` and the
table's schema by the end of the migration. Rejected: a generic SQL
passthrough cap (violates ADR-0010's "ops are shaped by services") and
typed-RPC growth (keeps the core domain-aware, contra ADR-0014). No data
migration guards the rename of the settings panels — nothing pre-plugin
runs in production, so `guild_plugins` rows are renamed freely.
