# Fork poise for runtime command replacement

ADR-0017 swaps plugin commands into the running bot, but upstream poise 0.7.0
fixes its command list at build time and exposes no runtime mutation API. All
six `pwr-bot` crates therefore use a FAZuH fork of poise, now pinned at git rev
`c8d5fed`. The fork adds `replace_commands`, which swaps in a host-built command
list at runtime and registers it with Discord, with a rollback path on failure.

Rejected: a supervised restart, because it needs a restart; a custom dispatch
shim outside poise, because poise would treat those commands as unknown.

Consequence: changing the poise source can re-resolve serenity's floating `next`
branch off the known-good pin (ADR-0001). After such a change, check the lockfile
with `cargo metadata --locked --offline` and keep the serenity entry unchanged.
The toolchain stays pinned to stable 1.98.

This fork now also carries the folded view macros from pwr-ext (ADR-0020).
