# Give every interaction exactly one acknowledger

After the TEA rewrite, two runtimes could answer the same click. The
global component handler acknowledged every interaction before routing
it — its only routing input was the plugin engine's session map, which
never contains Host messages — and the Host loop acknowledged the same
interaction again after handling it. Discord allows exactly one response
per interaction, so the second ack failed: a live `/about` Back click
logged `AlreadyResponded` and the "without an open session" debug line,
and the click did nothing.

We moved the ownership decision above both runtimes, into the
translation layer (`src/bot/translate.rs`). Every interactive view
message has at most one live owner, and the owner decides who
acknowledges:

- **Host session**: the Host loop claims its message for the loop's
  lifetime through an RAII `HostSession` guard
  (`TranslateLayer::host_session` at the top of `Host::run`,
  `src/bot/gui/rt.rs`). While the claim lives, the global event handler
  skips the message's component and modal interactions entirely
  (`handle_component_interaction` and `handle_modal_submit_interaction`
  in `src/bot/mod.rs`), and the Host acknowledges each interaction
  exactly once, after handling — except modal-triggering actions, where
  opening the modal is the response, and modal submissions, which the
  feature's poise modal task acknowledges while the session is live.
- **Plugin session, or no session**: the global handler acknowledges
  first — the plugin round trip can take most of Discord's three-second
  response window — and then routes the interaction to the plugin view
  engine, whose own session map answers live versus stale.

Each runtime keeps its own source of session truth: the layer tracks
Host-owned messages, the engine tracks plugin sessions. A Host claims
only the message its own render created, and a plugin registers only
the messages its responses created or adopted, so the two never
overlap.

The alternative was a shared ack path: one registry of live sessions
that both runtimes joined, so the global handler could consult it
instead of special-casing Host messages. That path couples Host teardown
to plugin registration timing, so we kept the claim-plus-skip instead.
The cost is a ghost window: the Host claim drops when the loop ends, and
the hub handoff registers the plugin session only after its morph edit,
so a click in between acknowledges, finds no engine session, and takes
the stale path. We accept that: the window is one morph edit wide, and
the message's view has just changed. The same acceptance covers a modal
submission that arrives after the session ends — the global handler
acks it, poise's own ack then fails with `AlreadyResponded`, and the
feature's modal task swallows both. That is stale-submission semantics,
by design.

ADR-0005 records the TEA runtime this layer sits above.
