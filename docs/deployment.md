# Deployment

How the bot stores data and writes logs while it runs. Install commands
live in the [README](../README.md). Variable names live in
[Configuration](configuration.md).

## Database

The bot uses PostgreSQL. It applies database migrations automatically at
startup. You do not run migration commands yourself.

## Logs

The bot writes logs to the `LOGS_PATH` directory (default: `logs/`).

## Docker volumes

Mount the `data/` and `logs/` directories into the container. This keeps
your data and logs when the container restarts. Both Docker examples in
the [README](../README.md#installation) mount these directories.
