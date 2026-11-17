# Deployment

How the bot stores data and writes logs while it runs. Install commands
live in the [README](../README.md). Variable names live in
[Configuration](configuration.md).

## Database

The bot uses PostgreSQL. It applies database migrations automatically at
startup. You do not run migration commands yourself.

## Logs

The bot writes rotating logs to the `LOGS_PATH` directory (default: `logs/`).
The latest log has a stable symlink named `pwr-bot.latest.log` in that
directory. Bare `pwr-bot` follows that file until you stop it. When the bot runs
as a detached service, stderr goes to the rolling log.

## Service

`pwr-bot serve --service` runs the bot as a detached service. Store the path to
the env file with `pwr-bot service install --env-file <path>`, then manage the
service with `pwr-bot service start`, `stop`, `restart`, and `status`. The
[README](../README.md#command-line-interface) lists the full CLI, including
`plugin`, `version`, and `debug paths`.

## Docker volumes

Mount the `data/` and `logs/` directories into the container. This keeps
your data and logs when the container restarts. Both Docker examples in
the [README](../README.md#installation) mount these directories.
