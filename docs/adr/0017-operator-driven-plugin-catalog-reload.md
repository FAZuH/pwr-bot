# Operator-driven plugin catalog reload

The operator can add, remove, and change a plugin grant by editing the local
`plugins.toml` while the bot runs. `pwr-bot plugin add/remove` is the supported
way to edit the catalog; direct edits are also honored. The bot polls `plugins.toml`
(2s poll, debounced) and reloads the catalog, grant snapshot, routes, manifests,
and command list. It validates everything first, then publishes the new grants,
the ArcSwap snapshot, and the fork command list in sequence, with rollback on
failure.

Authority stays where ADR-0016 put it: the operator's local config is the only
source of a grant. No Discord command and no plugin request grants a token; only
server access to the config file grants authority. This amends one point in
ADR-0016, which said the grant is snapshotted at startup and reused for every
spawn so authority never changes mid-flight. That point is reversed here: the
snapshot can be rebuilt at runtime, so authority can change while the bot runs.

Audit: the bot logs each apply and rollback, so a mid-flight change is visible
in the log.
