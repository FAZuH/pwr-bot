# pwr-bot

**Discord bot that sends feed update notifications to your DM or server.**

<hr>

<div align="center">
● <a href="#installation">Installation</a> ﻿ ● <a href="#discord-setup">Discord Setup</a> ﻿ ● <a href="#preview">Preview</a> ﻿ ● <a href="#usage">Usage</a><br>
● <a href="#features">Features</a> ﻿ ● <a href="#configuration">Configuration</a> ﻿ ● <a href="#command-registration">Command Registration</a> ﻿ ● <a href="#notes-and-tips">Notes and Tips</a><br>
● <a href="#docs">Docs</a> ﻿ ● <a href="#license">License</a>
</div>

## Installation

Create the Discord application first. See [Discord Setup](#discord-setup).

### Docker Compose (Recommended)

1. Clone the repository:
    ```sh
    git clone https://github.com/FAZuH/pwr-bot
    cd pwr-bot
    ```

2. Copy the example environment file and set your values (see [Configuration](#configuration)):
    ```sh
    cp .env-example .env
    ```

3. Start the bot. See [Usage](#usage).

### Docker Run

Prepare a `.env` file (see [Configuration](#configuration)), then run the container:

```sh
mkdir -p pwr-bot/data pwr-bot/logs && cd pwr-bot
docker run -d \
  --name pwr-bot \
  --restart unless-stopped \
  --env-file .env \
  -v $(pwd)/data:/app/data \
  -v $(pwd)/logs:/app/logs \
  ghcr.io/fazuh/pwr-bot:latest
```

To pass single variables instead of `--env-file`, use `-e DISCORD_TOKEN="..."` and `-e ADMIN_ID="..."`.

### Manual (Binary)

1. Download the binary for your platform from [GitHub Releases](https://github.com/FAZuH/pwr-bot/releases).
2. Put a copy of `.env-example` next to the binary. Rename it to `.env` and set your values (see [Configuration](#configuration)).
3. On Linux or macOS, make the binary executable:
    ```sh
    chmod +x pwr-bot
    ```

## Discord Setup

Create a Discord application before you run the bot:

1. Go to the [Discord Developer Portal](https://discord.com/developers/applications).
2. Click **New Application** and give the application a name.
3. Open the **Bot** tab:
    - Click **Reset Token**. Save the token as `DISCORD_TOKEN`.
    - Under **Privileged Gateway Intents**, enable **Message Content Intent**.
4. Open **OAuth2 → URL Generator**:
    - Select the scopes `bot` and `applications.commands`.
    - Select these bot permissions: `View Channels`, `Send Messages`, `Embed Links`, and `Read Message History`. The `!register` command needs `Read Message History`.
5. Open the generated URL and invite the bot to your server.
6. Open the **General Information** tab and copy the **Application ID**. You need it for the `ENABLE_AUTOREGISTER_CMD` feature.

## Preview

<img width="569" height="753" alt="image" src="https://github.com/user-attachments/assets/a04b4a51-be58-4c98-ac7f-c51967f8d5ad" />
<img width="555" height="327" alt="image" src="https://github.com/user-attachments/assets/7d551bb4-b919-49b0-a5d3-832438001f65" />
<img width="619" height="299" alt="image" src="https://github.com/user-attachments/assets/e13b24c8-084b-4800-b189-643c7560b56c" />
<img width="607" height="515" alt="image" src="https://github.com/user-attachments/assets/b1a4ac6a-07ed-4465-bfe1-c7d34292f43d" />

## Usage

Start the bot:

```sh
docker compose up -d    # Docker Compose install
```

```sh
./pwr-bot               # binary install
```

Check the log output (Docker Compose):

```sh
docker compose logs -f
```

After the bot starts, register the slash commands. See [Command Registration](#command-registration). Then try these commands in Discord:

- `/feed subscribe <links>` — subscribe to anime and manga feeds
- `/feed list` — show your subscriptions
- `/vc stats` — show your voice channel activity
- `/about` — show bot information

## Features

- **Feed Subscriptions:** Subscribe to updates from AniList, MangaDex, and Comick. The bot sends the updates to your DMs or to a server channel.
- **Voice Activity Tracking:** The bot measures the time members spend in voice channels. View server leaderboards with user rankings.
- **External Plugins:** Add features with plugins pinned in a catalog file. See [Plugin catalog](docs/plugins.md).

## Configuration

The bot reads its settings from a `.env` file. Copy `.env-example` and set at least `DISCORD_TOKEN` and `ADMIN_ID`. See the [configuration reference](docs/configuration.md) for all variables.

### Plugin catalog

The bot reads external plugins from `plugins.toml` at startup and lists them under `/plugins`. See [Plugin catalog](docs/plugins.md) for the file format and startup behavior.

## Command Registration

After the bot runs and joins your server, register the slash commands:

1. In a channel the bot can see, type `!register_owner`.
2. The bot responds with buttons.
3. Click **Register in guild** (immediate) or **Register globally** (can take up to one hour).

> [!note]
> The `!register_owner` command needs your Discord user ID to match `ADMIN_ID` in the environment file.
>
> Users in other servers with the "Administrator" or "Manage Server" permission can run `!register` or `!unregister`.

<img width="617" height="91" alt="image" src="https://github.com/user-attachments/assets/c0f508aa-e373-4df7-a574-01183eee4a98" />

## Notes and Tips

- **Database:** The bot uses PostgreSQL. It applies database migrations automatically at startup.
- **Logs:** The bot writes logs to the `LOGS_PATH` directory (default: `logs/`).
- **Docker volumes:** Mount the `data/` and `logs/` directories. This keeps your data and logs when the container restarts.

## Docs

- [Configuration reference](docs/configuration.md) — every environment variable
- [Plugin catalog](docs/plugins.md) — `plugins.toml` format and startup behavior
- [Architecture](docs/architecture.md) — system layers and module boundaries
- [Changelog](CHANGELOG.md) — release history
- [Issues](https://github.com/FAZuH/pwr-bot/issues) — bug reports and feature requests

## License

`pwr-bot` is distributed under the terms of the [MIT](https://spdx.org/licenses/MIT.html) license.
