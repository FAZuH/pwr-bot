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

**Amendment (#167):** The feed plugin embeds a copy of the old
`2026-04-29-122252-0000_initial_schema` migration under the same version string.
Diesel records only the version and run time, not a content checksum, so an
existing deployment that recorded that version skips the copy; a fresh
deployment runs core migrations first and the plugin copy declares only its
feed-owned tables. Feed copies a legacy `server_settings.feeds` value once into
its own storage, then reads only that storage; this compatibility read ends in
phase 6. Generic host enablement may still read `guild_plugins` during the
transition.
