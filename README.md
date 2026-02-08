<div style="text-align: center;">
● <a href="#features">Features</a> ﻿ ﻿ ﻿ ● <a href="#discord-setup">Discord Setup</a> ﻿ ﻿ ﻿ ● <a href="#installation--usage">Installation & Usage</a><br>
● <a href="#configuration">Configuration</a> ﻿ ﻿ ﻿ ● <a href="#command-registration">Command Registration</a> ﻿ ﻿ ﻿ ● <a href="#notes-and-tips">Notes and Tips</a><br>
● <a href="#bug-reports-and-feature-requests">Bug Reports and Feature Requests</a> ﻿ ﻿ ﻿ ● <a href="#license">License</a>
</div>

# pwr-bot

Discord bot that sends feed update notifications to your DM or server.

## Features

- **Anime and Manga Subscription:** Subscribe to updates from AniList, MangaDex, and Comick. Receive updates via Discord Direct Messages (DMs) or server channels.
- **Voice Channel Activity Tracking:** Track time spent in voice channels and view server-wide leaderboards with user rankings.
- **Lightning Fast:** *(Metrics based on v0.1.15)*
  - Application initialization: **~0.3s**
  - Bot initialization: **~2s**
- **Lightweight:** *(Metrics based on v0.1.15)*
  - Binary size: **21.2 MB**
  - Docker image: **46.4 MB**

<img width="569" height="753" alt="image" src="https://github.com/user-attachments/assets/a04b4a51-be58-4c98-ac7f-c51967f8d5ad" />
<img width="555" height="327" alt="image" src="https://github.com/user-attachments/assets/7d551bb4-b919-49b0-a5d3-832438001f65" />
<img width="619" height="299" alt="image" src="https://github.com/user-attachments/assets/e13b24c8-084b-4800-b189-643c7560b56c" />
<img width="607" height="515" alt="image" src="https://github.com/user-attachments/assets/b1a4ac6a-07ed-4465-bfe1-c7d34292f43d" />

## Discord Setup

Before running the bot, you need to create a Discord application:

1.  Go to the [Discord Developer Portal](https://discord.com/developers/applications).
2.  Create a **New Application** and give it a name.
3.  Navigate to the **Bot** tab:
    - Click **Reset Token** to get your `DISCORD_TOKEN`.
    - Under **Privileged Gateway Intents**, enable **Message Content Intent**.
4.  Navigate to **OAuth2 -> URL Generator**:
    - Select Scopes: `bot`, `applications.commands`.
    - Select Bot Permissions:
        - `View Channels`
        - `Send Messages`
        - `Embed Links`
        - `Read Message History` (Required for the `!register` command)
5.  Use the generated URL to invite the bot to your server.

## Installation & Usage

You can run this bot using Docker (recommended) or manually using the pre-compiled binary.

### Docker (Recommended)

#### Prerequisites

- [Docker](https://docs.docker.com/get-docker/)

#### Option 1: Docker Compose (Recommended)

1.  **Clone the repository**
    ```sh
    git clone https://github.com/FAZuH/pwr-bot
    cd pwr-bot
    ```

2.  **Configuration**
    Copy the example environment file and configure it (see [Configuration](#configuration)):
    ```sh
    cp .env-example .env
    # Edit .env with your text editor
    ```

3.  **Run**
    Start the bot in detached mode:
    ```sh
    docker compose up -d
    ```

#### Option 2: Docker Run

If you prefer to run the container directly without `docker compose` or cloning the full repository source code:

1.  **Prepare Directories**
    Create directories to persist data and logs:
    ```sh
    mkdir -p pwr-bot/data pwr-bot/logs
    cd pwr-bot
    ```

2.  **Run**
    Start the container (make sure you replace the placeholder values):
    
    ```sh
    docker run -d \
      --name pwr-bot \
      --restart unless-stopped \
      -v $(pwd)/data:/app/data \
      -v $(pwd)/logs:/app/logs \
      -e DISCORD_TOKEN="your_discord_token_here" \
      -e ADMIN_ID="your_admin_id_here" \
      ghcr.io/fazuh/pwr-bot:latest
    ```
    
    *Alternatively, you can use an env file:*
    ```sh
    # Assuming you have a .env file in the current directory
    docker run -d \
      --name pwr-bot \
      --restart unless-stopped \
      --env-file .env \
      -v $(pwd)/data:/app/data \
      -v $(pwd)/logs:/app/logs \
      ghcr.io/fazuh/pwr-bot:latest
    ```

### Manual (Binary)

#### Steps

1.  **Download the latest binary**
    Download the latest binary for your platform from the [GitHub Releases](https://github.com/FAZuH/pwr-bot/releases).

2.  **Configuration**
    Download the [.env-example](.env-example) file, rename it to `.env` in the same directory as the binary, and configure it with your text editor (see [Configuration](#configuration)).

3.  **Run**
    ```sh
    # If on Linux/macOS, make the binary executable first
    chmod +x pwr-bot
    ./pwr-bot
    ```

## Configuration

See `.env-example` for available configuration options.

| Variable | Description | Default |
|----------|-------------|---------|
| `DISCORD_TOKEN` | Your Discord bot token | **Required** |
| `ADMIN_ID` | Discord User ID for admin commands | **Required** |
| `POLL_INTERVAL` | Feed polling interval in seconds | `180` |
| `DATABASE_PATH` | Path to SQLite DB file | `./data/data.db` |
| `LOGS_PATH` | Directory for logs | `./logs` |
| `DATA_PATH` | Directory for logs | `./data` |
| `RUST_LOG` | Log level (e.g., `info`, `debug`. Read [here](https://rust-lang-nursery.github.io/rust-cookbook/development_tools/debugging/config_log.html) for more info) | `pwr_bot=info` |

## Command Registration

After the bot is running and invited to your server, you need to register the slash commands:

1.  In any channel the bot has access to, type `!register_owner`.
2.  The bot will respond with buttons to register the commands.
3.  Click **Register in guild** (immediate) or **Register globally** (may take up to an hour).

> [!note]
> Note that `!register_owner` command requires your Discord user ID to match environment variable's `ADMIN_ID`.
> 
> Users in other servers with "Administrator" or "Manage Server" permissions can simply run `!register` or `!unregister`.

<img width="617" height="91" alt="image" src="https://github.com/user-attachments/assets/c0f508aa-e373-4df7-a574-01183eee4a98" />

## Notes and Tips

- **Database:** The application uses SQLite. Migrations are handled automatically on startup.
- **Logs:** Application logs are stored in the configured `LOGS_PATH` (default: `logs/` directory).
- **Docker Volumes:** If you are using Docker, make sure `data/` and `logs/` are mounted to persist data and logs between restarts.

## Bug Reports and Feature Requests

You can report bugs or request for features on the [issue tracker](https://github.com/FAZuH/pwr-bot/issues).

## License

`pwr-bot` is distributed under the terms of the [MIT](https://spdx.org/licenses/MIT.html) license.
