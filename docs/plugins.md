# Plugin catalog

The bot uses external plugins. You pin these plugins in a catalog file,
`plugins.toml`. The bot reads the file at startup and lists its plugins
under `/plugins`.

## Location

The default path is `$DATA_PATH/plugins.toml`. Set `PLUGINS_TOML` to use
another path. The bot installs plugin binaries in `$PLUGINS_DIR` (default:
`$DATA_PATH/plugins`). See [Configuration](configuration.md) for these
variables.

## Format

See [`plugins.toml.example`](../plugins.toml.example) at the repository
root. Copy it to your catalog path and edit the entries. Each `[[plugins]]`
entry pins:

- the plugin name,
- an https download URL,
- the sha256 hash of the binary,
- the plugin manifest as a JSON string.

Set `auto_enable = true` to enable the plugin in every guild by default.

## Core plugins

The built-in plugins that ship with the host binary (`feed`, `voice`,
`welcome`) are not in the catalog. The host spawns the plugins
named in `CORE_PLUGINS` (a comma-separated list) at startup; each binary's
path resolves as [Configuration](configuration.md) documents. An unknown
name or a binary that fails to spawn is skipped with a warning and the
bot stays up.

## Feed

The `feed` core plugin owns feed subscriptions, delivery, and its settings
storage. It applies the migrations under
`crates/plugin/feed/migrations/` at startup and exposes the `/feed` command
group plus the `/feed-settings` panel command. Its single migration has a
unique version and creates only feed-owned tables with `IF NOT EXISTS`. Its own
service and repository write feed tables; the host bridges Discord and plugin
operations. The owner-only `/dump_db` command is the exception: the host
directly reads feed tables for its transitional database dump.

## Voice

The `voice` core plugin owns voice-session tracking, statistics, leaderboard
views, and voice settings. It subscribes to the host's `voice_state` and
`guild_create` events, stores sessions in its own PostgreSQL tables, keeps its
crash-recovery marker at `$DATA_PATH/voice_heartbeat`, and imports legacy voice
settings from `server_settings` once. It exposes the `/vc` command group and
the `/voice-settings` panel. The host supplies `host.get_config` and the
bounded `host.resolve_users` operation; voice does not use a host
voice-settings RPC. Its single migration has a unique version and creates only
voice-owned tables with `IF NOT EXISTS`. Runtime leaderboard images travel in
`ViewSpec.files` and are attached by the host on initial and interaction renders.

The protocol calls the declared host operation list `ops` and uses API
version `2`. A plugin's manifest may declare `voice_state`, `guild_create`, and
`view.timeout` event handlers.

## Missing or invalid catalog

The bot still starts when the catalog is missing or invalid. Plugin commands
stay absent. The bot logs the failure. The bot owner (`ADMIN_ID`) sees the
exact path and cause in `/plugins list`, and in the unknown-plugin errors of
`/plugins enable <name>` and `/plugins swap <name>`. Other users see "The
plugin catalog is empty."
