# pwr-bot

A Discord bot with feed subscriptions and voice channel tracking. One host
process runs the bot and its plugin subprocesses.

## Language

**Plugin**:
A subprocess that the host spawns to add commands and views to the bot. It
speaks JSON-lines over stdio through `pwr-plugin-protocol`, with a `"t"` tag
on every message. The plugins live in `crates/plugin/`, for example `hello`
and `settings`.
_Avoid_: extension, add-on

**Host**:
The pwr-bot monolith process. It spawns plugins, routes their Discord
interactions, and answers the `host.*` operations they call.
_Avoid_: server, daemon

**ViewSpec**:
The envelope a plugin returns to show a view,
`{data: Value, ephemeral: bool, view: Value}` in
`crates/pwr-plugin-protocol/src/view.rs`. `data` is the raw Discord
message payload, and `view` is plugin-owned opaque state that the host
stores and hands back on interactions. `ephemeral` is `true` when the
reply is visible only to the invoking user.
_Avoid_: view payload

**Gate**:
The host's validate-only check of `ViewSpec.data` at every raw-send
boundary: initial slash dispatch, component and modal re-render,
`host.open_view`, and the hub handoff's message morph. The host parses a
clone of the payload through
`pwr_ext::prelude::CreateMessageDe`, discards the parsed value, and sends
the original JSON unchanged. A failure is a `WireError` with kind
`InvalidView`, and the host sends, registers, and commits nothing. The
gate applies to every view, whether `view!` or the components library
authored it. See Verbatim send, Components library, and ADR-0003.
_Avoid_: validator, schema check

**Verbatim send**:
The rule that the host never rewrites plugin JSON on the wire. The send
paths transmit `ViewSpec.data` exactly as the plugin produced it, never
`into_canonical_value()` output.
_Avoid_: canonicalization

**Preview loop**:
The `crates/preview` bin behind `./dev.sh preview`. It spawns a plugin over
the same protocol port that the host uses. It captures the view that a
command returns, and `pwr-viewgen` renders the payload to HTML or PNG under
`.scratch/preview/`.
_Avoid_: preview tool, fixture renderer

**Components library**:
The reusable typed builders in `crates/pwr-poise-components`, rebuilt on
`pwr-ext`, kept for shared pieces the `view!` grammar does not fit
(pagination). Runtime assembly now composes `pwr-ext` `component!`/splices
plus the typed `view_support` builders inside a single `view!` literal:
plugins author the whole view with `view!` and splice runtime data (such
as the settings hub nav row) at its pinned positions, `Option`-gated on
discovery. No in-repo view is library-composed any more.
See ADR-0004.
_Avoid_: pwr-ext

**Translation Layer**:
The routing decision that gives every interactive view message exactly one
live session and one acknowledger. While a Host session owns a message the
global event handler skips its interactions and the Host loop answers them;
once no Host session owns it the global handler acknowledges first and
routes the interaction to the plugin view engine, whose own session map
answers live versus stale. See Host, Plugin, and ViewSpec.
_Avoid_: ack router, session registry

**Lifecycle message**:
The shared lifecycle pair every Host-driven feature speaks: `Start`, the
boot moment, and `Expired`, the view loop's timeout. Each feature's
message enum carries the pair as one wrapped `Lifecycle` variant from
`src/update/lifecycle.rs`, and `Lifecycle::handle` is the one common
handler: start runs nothing, expiry runs the feature's own expiry
behavior, such as persist-on-exit. It replaces the per-feature ad-hoc
exit messages. See Host and ADR-0005.
_Avoid_: exit message, boot message

**Host session vs Plugin session**:
The two owners an interactive view message can have. A host session is
a live TEA run: the Host loop drives one feature and owns the message
through the Translation Layer's claim, held for the loop's lifetime. A
plugin session is an entry in the plugin view engine's session map,
registered after the engine's response creates or adopts the message.
One message has at most one live owner at a time, and the owner answers
its interactions. The two maps stay separate, so the sessions never
overlap. See Translation Layer and ADR-0006.
_Avoid_: owned message, session claim

**Root Back**:
A Back press with an empty navigation history: the view on screen is
the root view, so Back dismisses it instead of navigating to a parent
frame. A public root view is deleted; an ephemeral one is left for the
user, because it belongs to the interaction that produced it. No host
feature returns the plain Back navigation today — every Back-capable
view hands off to the settings hub — so Root Back stays the navigation
walk's well-defined empty-history branch.
_Avoid_: exit Back, root dismissal

**Content placeholder vs deferred think**:
The two first-response shapes for an interaction. A content placeholder
is an immediate real message, such as "Loading…", that a later edit
replaces. A deferred think defers the interaction — Discord shows its
thinking state — and the payload then edits the original response in
place, so no placeholder message and no placeholder-then-followup pair
exists. The placeholder shape is retired: content set on the first
response survives a content-less edit and breaks components-V2 payloads
with 50035. The deferred think is the settled shape, and the error path
edits the original response too. See ADR-0007.
_Avoid_: loading message, placeholder reply

**Host op**:
One operation a plugin can call on the host over the protocol, written
`host.<name>` on the wire, such as `host.kv.get`. A plugin declares the
ops it calls in its hello `caps` list, and the host rejects an unknown
`host.*` op at spawn. See Host and ADR-0010.
_Avoid_: host command, host method

**Service RPC**:
A host op that mirrors one method of a host service, for example
`host.feed.get_settings`. The service stays the single source of truth,
and the plugin stays a thin client. Ops are shaped by services, never by
plugins: the host API grows only when the host domain grows. See Host op,
Panel plugin, and ADR-0010.
_Avoid_: bespoke op, plugin-shaped op

**Panel plugin**:
A plugin crate that owns one settings panel end to end — the interactive
settings view for one feature, such as feed settings, voice settings, or
welcome. It renders the view, answers its interactions, and reaches
service data through service RPCs. The panel migration turns the three
host-side panels into panel plugins, one crate each. See Service RPC and
ADR-0009.
_Avoid_: host panel, feature panel
