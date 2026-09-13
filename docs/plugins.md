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

## Missing or invalid catalog

The bot still starts when the catalog is missing or invalid. Plugin commands
stay absent. The bot logs the failure. The bot owner (`ADMIN_ID`) sees the
exact path and cause in `/plugins list`, and in the unknown-plugin errors of
`/plugins enable <name>` and `/plugins swap <name>`. Other users see "The
plugin catalog is empty."
