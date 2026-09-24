# Plugin data ownership

Plugins choose how they store their data; the host provides the
capabilities, not the schemas. The core Postgres connection capability
(`db_url`, already returned by `host.get_config`) is formalized as stable
protocol surface, and `data_path` is the sanctioned home for plugin files
(the voice session heartbeat is a file there, not a table). A plugin owns its
tables and can run its own embedded Diesel migrations at startup. Only the
feed plugin currently owns embedded migrations.
The shared `server_settings` table — an artifact of one process owning
three domains — splits into per-plugin settings storage, with each plugin
reading its legacy row on first startup; the core retains the shared table
only for the still-live Voice and Welcome settings and the feed legacy
import. Core also keeps `plugin_kv`, `guild_plugins`, and `bot_meta`, and
drops the remaining feed service and table ownership by the end of the
migration. Rejected: a generic SQL passthrough cap (violates ADR-0010's "ops
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
