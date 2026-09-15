# Use the lockfile to pin Serenity and Poise during development

Serenity and Poise use floating development branches, but the development
build must remain reproducible. We pin the resolved revisions in the committed
`Cargo.lock` file. We update these dependencies only with targeted commands
such as `cargo update -p serenity` or `cargo update -p poise`; we do not run a
bare `cargo update`.

Cargo cannot use a same-source `[patch]` entry to replace one revision with
another revision. Cargo also rejects a dependency that sets both `branch` and
`rev`. Therefore, manifest-level patches cannot provide this policy while the
dependencies keep their upstream branch sources. Revisit this decision when
the project moves from development branch dependencies to released versions.
