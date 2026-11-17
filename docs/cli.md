# CLI reference

`pwr-bot` is both the bot and an operator CLI. Run `pwr-bot --help` for the built-in help.

## Run the bot

```sh
pwr-bot                  # make sure the detached service is up, then follow the log and block
pwr-bot -n 50            # show the last 50 log lines before following
pwr-bot serve            # run the bot in the foreground
pwr-bot serve --service  # run the bot detached
```

Bare `pwr-bot` reuses a healthy service or starts one, then follows the rotating
log until you stop it. `serve` is the foreground command. The Dockerfile CMD and
the detached spawn both call it.

`serve --service` runs in its own session. It discards stdin and stdout, and
sends stderr to the rolling log. It writes a 0600 registration file at
`$XDG_STATE_HOME/pwr-bot/service.json` with `id`, `version`, `pid`, and
`logs_path`. A watchdog re-reads the file.

Secrets come from the env file you install with `service install`. They never
enter the process arguments.

## Service commands

```sh
pwr-bot service install --env-file <path>   # store the env file path in private config
pwr-bot service start                       # start the detached service
pwr-bot service stop                        # stop it
pwr-bot service restart                     # restart it
pwr-bot service status                      # show whether it runs
```

`install` stores the path in a 0600 private config file. The spawn reads the
file and injects its variables. `stop` sends SIGTERM, then SIGKILL, and removes
the registration only if the service still owns it.

## Plugin commands

```sh
pwr-bot plugin add --name hello --url https://example.com/hello.tar.gz \
  --sha256 <digest> --manifest ./manifest.json --auto-enable
pwr-bot plugin list                    # print the catalog
pwr-bot plugin remove --name hello     # drop the entry, leave the binary on disk
```

`add` takes these flags:

- `--name`, `--url`, `--sha256`, `--manifest`: the plugin name, download URL, binary digest, and manifest file.
- `--grant-token`: grants the plugin the bot's Discord token. It must agree with the manifest `requires: ["discord_token"]` declaration in both directions. The add fails if only one of them is set.
- `--auto-enable`: enables the plugin when the bot starts.

`list` prints each catalog entry with its URL, digest, `auto_enable`, and
`discord_token` flags. `remove --name <name>` drops the catalog entry and leaves
the installed binary on disk.

## Other commands

```sh
pwr-bot version        # print the package version
pwr-bot debug paths    # print resolved config, data, log, and registration paths
```

## Logs

The bot writes rotating logs to `LOGS_PATH` (default `./logs`) as
`pwr-bot.<date>.log`. A stable `pwr-bot.latest.log` symlink points at the newest
file. Bare `pwr-bot` follows the symlink. When the bot runs detached, stderr
goes to the same rolling log.
