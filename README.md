<div align="center">

# pwr-bot

**Discord bot that sends feed update notifications to your DM or server.**

</div>

<hr>

<div align="center">
● <a href="#installation">Installation</a> ﻿ ● <a href="#preview">Preview</a> ﻿ ● <a href="#usage">Usage</a> ﻿ ● <a href="#features">Features</a><br>
● <a href="#docs">Docs</a> ﻿ ● <a href="#license">License</a>
</div>

## Installation

Create the Discord application and invite the bot first — see [Discord Setup](docs/discord-setup.md).

### Docker Compose (Recommended)

1. Clone the repository:
    ```sh
    git clone https://github.com/FAZuH/pwr-bot
    cd pwr-bot
    ```

2. Copy the example environment file and set your values (see [Configuration reference](docs/configuration.md)):
    ```sh
    cp .env-example .env
    ```

3. Start the bot. See [Usage](#usage).

### Docker Run

Prepare a `.env` file (see [Configuration reference](docs/configuration.md)), then run the container:

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
2. Put a copy of `.env-example` next to the binary. Rename it to `.env` and set your values (see [Configuration reference](docs/configuration.md)).
3. On Linux or macOS, make the binary executable:
    ```sh
    chmod +x pwr-bot
    ```

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

Once the bot runs, register the slash commands — see [Command registration](docs/discord-setup.md#command-registration). Then try these commands in Discord:

- `/feed subscribe <links>` — subscribe to anime and manga feeds
- `/feed list` — show your subscriptions
- `/vc stats` — show your voice channel activity
- `/about` — show bot information

## Features

- **Feed Subscriptions:** Subscribe to updates from AniList, MangaDex, and Comick. The bot sends the updates to your DMs or to a server channel.
- **Voice Activity Tracking:** The bot measures the time members spend in voice channels. View server leaderboards with user rankings.
- **External Plugins:** Add features with plugins pinned in a catalog file. See [Plugin catalog](docs/plugins.md).

## Docs

- [Discord Setup](docs/discord-setup.md) — Developer Portal, intents, invite URL, and command registration
- [Configuration reference](docs/configuration.md) — every environment variable
- [Plugin catalog](docs/plugins.md) — `plugins.toml` format and startup behavior
- [Deployment](docs/deployment.md) — database migrations, logs, and Docker volumes
- [Architecture](docs/architecture.md) — system layers and module boundaries
- [Changelog](CHANGELOG.md) — release history
- [Issues](https://github.com/FAZuH/pwr-bot/issues) — bug reports and feature requests

## License

`pwr-bot` is distributed under the terms of the [MIT](https://spdx.org/licenses/MIT.html) license.
