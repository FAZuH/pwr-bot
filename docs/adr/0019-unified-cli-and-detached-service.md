# Unified CLI and detached service

pwr-bot becomes one binary with subcommands. The CLI folds in the former
`pwr-plugin` tool and gives operators one entry point to run the bot and manage
its background instance. A detached process follows OpenCode's mechanism, so the
bot runs the same way on a host and inside the container without a service
manager.

Bare `pwr-bot` makes sure the service is up, then follows the rotating log and
blocks. `pwr-bot serve` runs the bot in the foreground. It is the body of the
current `main.rs` and the target of the Dockerfile CMD and the detached spawn.
`pwr-bot serve --service` runs the detached service. `pwr-bot plugin
add|list|remove` carries the former `pwr-plugin` commands. `pwr-bot service
start|stop|restart|status` act on the service pid, and `pwr-bot service install
--env-file <path>` stores the env file path in private config. `pwr-bot version`
prints the package version, and `pwr-bot debug paths` prints the resolved
config, data, log, and registration paths.

The detached service starts in its own session. It discards stdin and stdout,
and sends stderr to the rolling log. Its lease is a 0600 registration file at
`$XDG_STATE_HOME/pwr-bot/service.json` holding `id`, `version`, `pid`, and
`logs_path`. The `logs_path` value is absolute, and the tail command reads it
from the lease. The service writes the file through a temporary file and
rename, and a watchdog re-reads it. Clients check pid liveness before they
attach. A recycled pid is not detected, so clients trust the lease for process
identity and do not check the process start time. `service stop` sends SIGTERM,
then SIGKILL, and removes the registration only if the process still owns it.
A client reuses a healthy service, replaces one whose version does not match,
and never kills a slow contender. The spawn reads the env file named in private
config and injects its variables, so secrets never enter argv.

## Considered options

- systemd user unit: rejected. It is Linux-only, it cannot run inside the
  Docker container, and OpenCode shows that detaching a process is enough.
- HTTP API with `api` and `config` subcommands: deferred. No local server exists
  yet, and plugin reload already works through `plugins.toml`.
- Remote deploy control (faz-lab): out of scope. It stays in `service-ops.sh`.

## Consequences

- No crash auto-restart in this change. Boot persistence needs a supervisor.
- No Windows support for the detached spawn.
- The rotating log file already exists. It gains a stable latest-file symlink
  that the tail command follows.
- The plugin CLI ships inside `pwr-bot` under issue #177. No separate CLI binary ships.
