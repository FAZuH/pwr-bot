# Add modal support to the plugin protocol

The plugin protocol has no modal op. A plugin view can send, edit, defer,
and acknowledge, but it cannot open a modal or receive a submission. A
plugin that needs a modal has no path today.

The welcome panel is the one host view that uses one. Its `open_modal`
hook (`src/bot/gui/welcome.rs`) is the only such impl in the host gui.
Two actions open a modal — add a welcome message and set the primary
color. The hook spawns a poise modal task that awaits the submission and
delivers it as a plain `Msg` on the view channel. The host loop consumes
the interaction without an ack: opening the modal is itself the response
(ADR-0007).

Welcome migrates fully in phase 1 of the panel migration (ADR-0009), so
the capability must land with it. If the capability is missing, welcome
migrates partially — its modal actions stay behind in the host runtime.

We decided to add modal support to the plugin protocol: a host op that
opens a modal, plus plugin-side handling of the modal submission that
follows. The exact op shape is a design-pass question. The decision here
is the capability and the constraint it must respect.

The constraint is the modal collector. The host's modal collector is
author-keyed, not message-keyed (`ModalInteractionCollector` in
`src/bot/view/mod.rs`). It filters by `author_id` and a timeout, and any
modal that the author submits in that window matches the collector. The
component collector is different: it filters by message id. So a modal
submission cannot be routed to a session by its message alone. The
design must route a modal submission to the owning plugin session, with
the interaction id and token that the plugin already knows how to carry
(the `interaction_id` + `token` pair, as `host.defer` and
`host.acknowledge` args take today, `src/plugin/host.rs`).

One consequence raises an open question: welcome's image generation
crosses the wire after migration. The host renders a welcome-card preview
image from the live settings (`WelcomeImageGenerator`,
`src/bot/gui/welcome.rs`). The plugin must get that image somehow.
Whether the service RPC carries the bytes or a path that the plugin
resolves is an open question for the design pass, not settled here.
ADR-0010 records the RPC seam it will ride.

ADR-0007 records the first-response shape the modal op must fit: a
modal-triggering action skips the acknowledge, because opening the modal
is the response.
