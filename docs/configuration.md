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
| `CORE_PLUGINS` | Comma-separated core plugin names, spawned at startup in order. See [Plugin catalog](plugins.md). | empty — no core plugins |
| `FEED_PLUGIN_PATH` | Path to the feed core plugin binary. | `./feed` next to the bot binary, then `$DATA_PATH/feed` |
| `VOICE_PLUGIN_PATH` | Path to the voice core plugin binary. | `./voice` next to the bot binary, then `$DATA_PATH/voice` |
| `WELCOME_PLUGIN_PATH` | Path to the welcome core plugin binary. | `./welcome` next to the bot binary, then `$DATA_PATH/welcome` |
| `ENABLE_AUTOREGISTER_CMD` | Enable command autoregistration. | `true` |
| `DISCORD_APPLICATION_ID` | Discord application ID. Required for command autoregistration. | `1234567890` |
| `RUST_LOG` | Log level, for example `info` or `debug`. See [log filter syntax](https://rust-lang-nursery.github.io/rust-cookbook/development_tools/debugging/config_log.html). | `pwr_bot=info` |
