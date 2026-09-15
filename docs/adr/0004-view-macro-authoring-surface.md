# Author plugin views with the pwr-ext view! macro

Plugin views started as hand-rolled `json!` bodies. The `view!` macro from
`pwr-ext` emits a typed serenity `CreateMessage` with compile-time literal
laws. Hand-written JSON carries no such guarantee. We made plugins the
first adoption surface for `view!`, ahead of the host monolith views in
`src/bot/view/`.

A plugin authors a fixed view with `view!`, then serializes the
`CreateMessage` to JSON as `ViewSpec.data`. The `hello` view and the
settings `about_view` (`crates/plugin/settings/src/main.rs`) follow this
pattern. Serenity always serializes the fields the gate requires, such as
`tts` and `enforce_nonce`, so a `view!`-authored envelope needs no extra
fields to pass the gate.

`crates/pwr-poise-components` stays as the reusable components library,
rebuilt on `pwr-ext` typed construction. The library is not redundant with
`view!`: a view that needs runtime assembly composes library builders
instead of one macro literal. The settings hub keeps this split. Its nav
row holds zero to N discovery-driven buttons inside an otherwise fixed
container. The `view!` grammar has no runtime children splicing, no
conditionals, and no standalone component emission. The gap is documented
inline at the `view_data` call site in the settings hub. A future `pwr-ext`
grammar extension can unblock full hub adoption, as a separate multi-repo
change.

Typed construction moves errors to authoring time, and the gate receives
envelopes that already match the schema. The cost is the two authoring
modes. A plugin picks `view!` for fixed views and the components library
for runtime assembly. The hub keeps library composition until the grammar
grows.

## Update (2026-09-02)

The pwr-ext `view!` grammar extension landed in pwr-ext `5a5b500`: runtime
splices, `Option` conditionals, and `component!`. The settings hub migrated
onto it, so the "grammar gap" narrative above and the "hub keeps library
composition" line are superseded.

The hub is now a single `view!` literal with runtime splices at their
pinned positions: the config buttons, the whole toggle row, and the
`Option`-gated nav row. The components library's runtime-assembly role is
superseded by `component!`/splices; the crate stays as the reusable library
for shared pieces the grammar still does not fit (pagination, for example).
