# Fold pwr-ext into the poise fork

pwr-bot depended on pwr-ext, a separate repository at v0.1.0, for the `view!`
and `component!` macros, the view spec deserializers, and the `view_support`
builders. pwr-bot is its only consumer in this repository. Upstream poise and
serenity provide none of this; serenity explicitly deferred Deserialize
builders. We now maintain a poise fork, so a second repository dependency is
avoidable.

We folded the pwr-ext library into the poise crate as `poise::ext::*` and
`poise::view_support`. Its proc macros moved into a third path-dependent crate
inside the fork workspace, re-exported from poise under the `view-macros`
feature. Every emitted path resolves through poise. The fork has one source at
one pinned commit, `c8d5fed`, with no tags. pwr-bot drops pwr-ext and enables
`view-macros` on all six crates.

## Considered options

- Vendor pwr-ext as a workspace member that keeps the `pwr_ext` crate name:
  rejected. That is not a full fold and keeps two crate names and the shim
  problem.
- Merge the proc macros into `poise_macros`: rejected. That needs a syn 2 to
  syn 3 port of about 3,200 lines for no user-visible gain.
- Wait for upstream: rejected. No upstream plan exists.

## Consequences

- External `pwr-viewgen` still imports `pwr_ext::CreateMessageDe`. It needs a
  separate update, out of scope here.
- The 46 upstream poise commits stay deferred. The fork remains on the
  runtime-command-swap lineage with the ArcSwap patch.
- The trybuild UI suite was ported to the fork.
- The serenity pin `f2b66488` is unchanged.
