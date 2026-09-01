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
