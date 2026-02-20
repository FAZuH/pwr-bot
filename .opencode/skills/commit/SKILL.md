---
name: commit
description: Create git commits following the project's Conventional Commits specification. Handles commit message formatting, types, scopes, user-facing commit detection, and CI skip patterns.
---

# Commit Conventions

This project follows the **Conventional Commits** specification for clear and consistent commit history.

## Commit Message Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

## Commit Types

| Type | Description |
|------|-------------|
| `feat` | New feature |
| `ui` | Changes to user interface |
| `fix` | Bug fix |
| `docs` | Documentation only changes |
| `style` | Code style changes (formatting, semicolons, etc.) |
| `refactor` | Code refactoring |
| `perf` | Performance improvements |
| `test` | Adding or updating tests |
| `chore` | Build process, dependencies, etc. |
| `build` | Build system or dependency changes |

## User-Facing Commits

For user-facing commits (visible to end users), add `u_` prefix to the type:

- `u_feat` - New user-facing feature
- `u_ui` - User interface changes
- `u_fix` - User-facing bug fix
- `u_refactor` - User-facing refactor

**Examples:**
- `u_feat(bot): Add /vc stats command`
- `u_ui(bot): Show time range as relative timestamps in /vc leaderboard`
- `u_fix(bot): Fix swapped unsubscribe and undo button`

**Why?** The CI detects `u_` prefix to generate user-facing changelogs.

## Scopes

Use scopes to indicate which part of the codebase changed:

- `(bot)` - Discord bot commands
- `(db)` - Database/repository layer
- `(voice)` - Voice tracking features
- `(feed)` - Feed subscription features

**Examples:**
- `feat(voice): Add /vc stats command`
- `fix(db): Correct SQL query for daily average calculation`
- `refactor(bot): Restructure commands module file tree`

## Guidelines

### Subject Line Rules
- **Capitalize** the first letter (unless it is strictly lowercase like a variable name)
- Use **present tense** ("Add feature" not "Added feature")
- Use **imperative mood** ("Move cursor to..." not "Moves cursor to...")
- Keep subject under 50 characters when possible
- No period at end of subject

### Body Rules
- Separate subject from body with a blank line
- Use body to explain **what** and **why**, not **how**
- Include motivation for change and contrast with previous behavior
- Use bullet points for multiple changes (prefix with `-`)

### Footer Rules
- Reference issues: `Closes #123`, `Fixes #456`
- CI skip: `[skip ci]` for documentation/format-only commits

## Examples

### Feature Commit
```
feat(voice): Add /vc stats command with contribution grid

Add voice activity statistics command that displays historical
data using GitHub-style contribution heatmaps.

- Support user and guild stats views
- Add time range selection
- Display total time, average, streak, and most active day
```

### Bug Fix Commit
```
fix(db): Correct SQL query for daily average calculation

The subquery was not properly aliased, causing column reference
errors in SQLite.
```

### User-Facing UI Commit
```
u_ui(bot): Show time range as relative timestamps in /vc leaderboard

Display time ranges using Discord relative timestamps (e.g., "3 days ago")
instead of absolute dates for better user experience.
```

### Refactor Commit
```
refactor(bot): Restructure commands module file tree

Move command implementations to individual files in src/bot/commands/
for better organization and easier maintenance.
```

### Documentation Commit
```
docs: Update dev docs [skip ci]

Update AGENTS.md with new commit conventions and add skill
documentation for UI views and database schema changes.
```

### Chore Commit
```
chore: Bump dependency versions

Update serde and tokio to latest stable versions.
```

## CI Integration

### Skip CI Commits
Use `[skip ci]` footer for commits that don't need CI validation:
- Documentation only changes
- Code formatting changes
- Merge commits

```
docs: Update README [skip ci]
style: Format code [skip ci]
```

### CI Detection
The CI automatically detects user-facing commits (starting with `u_`) for changelog generation.

## Creating Commits

### Using Git Directly

```bash
# Stage changes
git add -A

# Create commit with proper format
git commit -m "feat(bot): Add new command"

# Or use full format with body
git commit -m "feat(bot): Add new command" -m "Add description of changes"
```

### Commit Message Validation

Before committing, verify:
- [ ] Type is correct (feat, fix, refactor, etc.)
- [ ] Scope is appropriate (bot, db, voice, feed)
- [ ] Subject is capitalized and uses imperative mood
- [ ] User-facing changes have `u_` prefix
- [ ] Documentation-only commits have `[skip ci]`

## Common Mistakes

| Mistake | Correction |
|---------|------------|
| `feat: add feature` | `feat(bot): Add feature` |
| `Added new feature` | `feat(bot): Add new feature` |
| `fix(bot): fixed bug` | `fix(bot): Fix bug` |
| `refactor: Refactored code` | `refactor(bot): Refactor code structure` |
| User-facing without `u_` | `u_feat(bot): Add feature` |

## Git Log Reference

Recent commits in this project follow these patterns:
```
72b7070 docs: Update dev docs [skip ci]
16604d4 refactor(bot): Restructure commands module file tree
d63313b u_feat(bot): Show package version to bot activity [skip ci]
7d96bf2 fix(bot): Improve UI and error handling
fbe737b u_feat(bot): Add /vc stats command
110bd05 u_fix(bot): Fix swapped unsubscribe and undo button
0efff86 u_feat(bot):
```
 Add feed unsubscribe buttons