# Configuration

Set the environment variables in a `.env` file. Start from `.env-example`
in the repository root. The bot reads the file at startup.

| Variable | Description | Default |
|----------|-------------|---------|
| `DISCORD_TOKEN` | Discord bot token. Required. | — |
| `ADMIN_ID` | Discord user ID of the bot owner. Required for admin commands. | — |
| `POLL_INTERVAL` | Feed polling interval in seconds. | `180` |
| `DB_URL` | PostgreSQL connection URL. | `postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot` |
| `DB_PASS` | PostgreSQL password. | `pwr_bot` |
| `DB_USER` | PostgreSQL username. | `pwr_bot` |
| `DB_NAME` | PostgreSQL database name. | `pwr_bot` |
| `LOGS_PATH` | Directory for log files. | `./logs` |
| `DATA_PATH` | Directory for data files. | `./data` |
| `PLUGINS_TOML` | Plugin catalog file. See [Plugin catalog](plugins.md). | `$DATA_PATH/plugins.toml` |
| `PLUGINS_DIR` | Directory where the bot installs plugin binaries. | `$DATA_PATH/plugins` |
| `SETTINGS_PLUGIN_PATH` | Path to the settings core plugin binary. | `./settings` next to the bot binary, then `$DATA_PATH/settings` |
| `FEED_SETTINGS_PLUGIN_PATH` | Path to the feed settings panel plugin binary. | `./feed-settings` next to the bot binary, then `$DATA_PATH/feed-settings` |
| `VOICE_SETTINGS_PLUGIN_PATH` | Path to the voice settings panel plugin binary. | `./voice-settings` next to the bot binary, then `$DATA_PATH/voice-settings` |
| `ENABLE_VOICE_TRACKING` | Enable voice channel tracking and heartbeat. | `true` |
| `ENABLE_FEED_PUBLISHER` | Enable feed polling and publishing. | `true` |
| `ENABLE_AUTOREGISTER_CMD` | Enable command autoregistration. | `true` |
| `DISCORD_APPLICATION_ID` | Discord application ID. Required for command autoregistration. | `1234567890` |
| `RUST_LOG` | Log level, for example `info` or `debug`. See [log filter syntax](https://rust-lang-nursery.github.io/rust-cookbook/development_tools/debugging/config_log.html). | `pwr_bot=info` |
