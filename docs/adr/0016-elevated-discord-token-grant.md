# Elevated Discord token grant

A plugin that needs Discord authority beyond the shaped `host.*` op surface can be given the bot's
own token. The mechanism is built now even though no first-party plugin needs it, because the
first-party set is closed and the future set is not: third-party plugins, code the operator did not
write and for which the host cannot shape ops, will need authority that no op set covers.
First-party need routes to a shaped op instead (ADR-0010), which is the cheaper and safer answer
whenever it exists.

The operator's configuration is the only authority. A plugin cannot escalate itself: no manifest
field, no `host.*` call, and no wire message grants a token. The grant rides on the catalog entry
rather than a separate table — `[[plugins]]` gains `discord_token = true` alongside the existing
`name`, `url`, `sha256`, `manifest`, and `auto_enable` (`CatalogEntry` in `src/plugin/install.rs`).

The token is injected through the child process environment at spawn and never rides the wire: no
runtime request, no new op, no `API_VERSION` change, which stays 2. A runtime handshake grant, where
the plugin asks and the host answers with the token, was rejected on 2026-09-24 — it keeps the token
in a message the host logs and a plugin can echo into a view, and it makes authority a per-call
decision instead of a per-install one. "On request" meant the operator's request, recorded in
`plugins.toml`, never the plugin's ask. A generic REST passthrough op is rejected for the reason
ADR-0010 gives for every op: ops are shaped by services, and a token-carrying passthrough is shaped
by a plugin's wish.

**Environment invariant:** the child environment is an explicit allowlist, not
inherit-everything-minus-one. It carries `PATH` plus whatever the grant adds
(`src/plugin/mod.rs:351-360`). The leak it closes was that spawning removed only
`DISCORD_TOKEN` from an otherwise inherited environment, so `DB_URL`,
`DISCORD_APPLICATION_ID`, and `ADMIN_ID` crossed the process seam. A per-plugin `env` map on the
catalog entry is rejected — it reopens the hole by letting the operator hand a variable to any
binary, and it becomes a second injection surface beside the grant. No shipped plugin needs a
variable, because `host.get_config` already delivers `db_url`, `data_path`, and `poll_interval`
(`src/plugin/host.rs:431-438`). The same decision deletes `ENABLE_FEED_PUBLISHER` and
`publisher_enabled()`: the feed plugin held the only environment read in any plugin binary, nothing
in the repository set it, and it defaulted to on. Feed config now arrives over `host.get_config`
(`crates/plugin/feed/src/main.rs:413-458`), and feed publishing is toggled by stopping and starting
the plugin.

The host verifies the binary's sha256 at spawn against the digest already in the catalog entry.
Authority attaches to bytes, not to a name, so a swapped binary is refused at the seam instead of
inheriting a grant made for the bytes the operator reviewed. The check and the exec are not atomic:
the digest is read and verified, then the binary is spawned, so an attacker who can write the
installed file in between is outside what this seam defends.

The manifest gains `requires: ["discord_token"]` — a list over a closed vocabulary, validated at
hello the way `validate_ops` rejects an unknown `host.*` entry today. It is additive on
`#[serde(default)]`, so a plugin built before the field existed still loads and simply declares
nothing.

**Grant invariant:** the declaration and the grant must agree in both directions, and for a catalog
plugin the check runs before the child environment is built. A grantable plugin is by definition a
catalog entry, because the host already holds the embedded catalog manifest before spawn. A plugin
that declares the need without a grant is refused; a plugin that is granted without declaring the
need is refused. Either disagreement is a refusal, and both directions are checked because a grant
nobody declared is authority a plugin holds on a technicality. The declaration half here is the
operator's assertion in the catalog entry, not the plugin's own handshake manifest, so a binary
whose manifest disagrees with the catalog is not caught by this check.

Consequence, and it is intended: `CORE_PLUGINS` plugins cannot be granted. There the host knows only
a name and a path (`src/config.rs:36`), so the declaration half is unknowable before spawn and the
AND cannot be enforced before the environment exists. A core plugin that declares the need anyway is
therefore refused at the handshake instead: the grant is still `none`, and the declaration the host
can finally read disagrees with it. A first-party plugin that ever needs Discord authority therefore
either gains a shaped op or moves into the catalog, where it becomes pinned and digest-verified like
any other granted plugin. The pressure is deliberate — a grant is a statement about a reviewed
binary, and a core plugin is only a binary on disk. The trade-off, stated plainly: because only
catalog plugins are grantable, granting a first-party plugin means moving it into the catalog, which
turns "binary on disk" into "downloaded and digest-verified".

Refusal is an error and the plugin does not run; the bot stays up. That is existing behavior for a
failed core-plugin spawn at `src/bot/mod.rs:253`, where a missing binary is already logged and
skipped.

The grant is snapshotted at startup and reused for every spawn, including respawns after a crash, so
authority never changes mid-flight.

Audit: one info-level line per spawn naming the plugin, the binary, the grant decision, and the
digest result; `/plugins list` shows each plugin's authority. A plugin that declares a need nobody
granted it is refused, so there is no such plugin to warn about afterwards.

The catalog parses strictly. An unknown key is a startup error, the digest must be 64 hex characters
(case-insensitive, and normalised once when the startup snapshot is built, so the spawn seam compares
without re-deriving the trim and case rules), and the name must be non-empty — the same treatment
`validate_entry` already gives entries in the same file. A typo in a grant line fails the whole
catalog at boot rather than producing a silently ungranted plugin.

A separate `[grants]` table keyed by name is rejected: its only justification was serving both spawn
paths, and the catalog-only rule removed the second path. Grants for `CORE_PLUGINS` plugins are
rejected for the reason above. A `--manifest` pre-read flag in the plugin SDK, which would close the
pre-spawn window on both paths, buys a second exec and a new SDK surface against an attacker who
could already drop a binary in the plugins directory.

This amends ADR-0014 (thin loader) and is adjacent to ADR-0015 (data ownership).
