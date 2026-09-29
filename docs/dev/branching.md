# Branching model

How branches, PRs, and releases relate in this repo.

## Branch roles

```
main ──────●──────●──────────►   stable; always releasable
              ↑        ↑
development ──●──●──●──●─────►   integration; PRs land here
                 ↑
fix/xyz ─────────●               short-lived; PRs target development
```

- **`main`** — the stable working branch. Always in a releasable state. It
  tracks the latest release.
- **Integration branch** — where development accumulates between releases.
  Conventionally **`development`**. A large migration or long-running effort
  may instead use a **version branch** named for the next release (for
  example `v0.5`) — same role, same rules.
- **Feature / fix / worktree branches** — short-lived, cut from and merged
  back into the active integration branch. Deleted after merge.

## Rules

1. **PRs target the active integration branch**, never `main`. The one
   exception is a hotfix (rule 4).
2. **Stabilization merge**: when the integration branch is stable enough to
   release, it merges into `main` with one PR. Pair it with a version bump
   commit (`chore!(minor)` or `chore!(major)`) so the release tooling cuts
   the right version. This merge is the "push a new version" moment —
   autopromote (main → release) fires on it.
3. **Hotfixes**: land on `main` first, then merge back into the active
   integration branch immediately. This is the only rule that prevents
   drift between a long-lived integration branch and `main` — skip it and
   the stabilization merge becomes a conflict festival.
4. **Merge-only**: every PR merge is a real merge commit
   (`merge_method=merge`). Never squash, never rebase-merge. See
   `docs/dev/commit-changelog.md`.

## CI behavior (no workflow changes needed)

- `pull-request.yml` (Rust Format & Test) runs on **every PR regardless of
  base branch**, so PRs into an integration branch get the full gate.
- `autopromote.yml` fires only on pushes to `main` — integration-branch
  pushes never touch the release line.
- `release.yml` fires only on pushes to the `release` branch.

## Example: a versioned effort

```
main        ──●────────────────────────●──►  stays at current release
               │                        ↑
v0.5        ──●──●──●──●──●──●──●──●────┘     integration; 7 phase PRs
               ↑  ↑
feat/p1-renames┘  feat/p2-send-caps         worktree branches; PRs → v0.5
```

Each phase PR keeps the integration branch green. When all phases land and
the suite is stable, one stabilization PR merges `v0.5` into `main` with the
version bump, and the release follows.
