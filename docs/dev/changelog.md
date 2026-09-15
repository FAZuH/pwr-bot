# Changelog Guide

The `CHANGELOG.md` structure follows
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/). Read that
spec first; this guide adds the house style on top of it.

The release workflow does not generate the changelog. On release, the topmost
`## [Unreleased]` section is renamed to `## <version> (<date>)` — e.g.
`## 0.1.3 (2026-08-14)`. If the section is missing, the tooling inserts one
before renaming it, so the release body is never a previous release's notes.
Add and populate the topmost `## [Unreleased]` during development with the
changes shipping in the next release. Without entries, the release body shows
only the version heading.

## What belongs in the changelog

Only user-visible changes. A change belongs when it alters how the software
behaves for the user: a new feature, a changed behavior, a fixed bug, a
removed capability.

Internal work stays out: refactors, dependency bumps, tooling, CI, test, and
documentation changes. When in doubt, leave it out.

## Wording

Write each entry as a plain statement about user-visible behavior. Past
tense. Short, concrete sentences. No implementation detail, no jargon, no
commit hashes, no issue or pull-request links.

Name the change first, then what changed about it. Do not explain how it
works or why.

Examples:

```
- Fixed the Golden Avia flying away from you under certain conditions
- Added up to level 25 Potions of Healing to the Elkurn Potion Merchant
- Reduced the health and damage of all enemies, including the boss
- Removed the Cindercurse set pieces from the normal loot pool
- Fixed enemies not having any animations on spawn
- Enemies stuck in the water will instead die
```

## Brevity

The examples above are the ceiling for entry length: one short clause,
under 15 words. State the outcome, not the mechanism. Cut any clause that
explains how, why, or what it prevents. If the entry still reads correctly
without a clause, that clause was detail.

## Deadwood

Cut deadwood: wording that can be omitted without losing meaning. A word or
phrase is deadwood when the entry reads correctly without it. Apply the same
test to whole clauses: if the clause adds nothing, cut it.

Common deadwood:

- Articles: `the`, `a`, `an`
- Filler qualifiers: `successfully`, `actually`, `effectively`, `simply`
- Redundant modifiers: `new` (the change word `Added` already implies it),
  `currently`, `previously`
- Empty phrases: `in order to` (say `to`), `due to the fact that` (say
  `because`), `at this point in time` (say `now`), `the process of`
- Useless fillers: `that`, `just`, `very`, `quite`, `basically`, `etc.`

Articles are deadwood unless removing them breaks the sentence. Compare:

- "Added sorting to **the** absensi **table**" → "Added sorting to absensi
  table"
- "Added **a** create report card **on the** Laporan **page**" → "Added
  create report card on Laporan page"
- "Removed **the** schedule **select** column **from the** absensi **add**
  form" → "Removed schedule select column from absensi add form"
- "Removed **the** single **and** bulk delete actions **from the** Murid
  **page**" → "Removed single and bulk delete actions from Murid page"

Keep the article when it is needed to parse correctly, e.g. before a
countable noun phrase that cannot stand alone.

Rewrite, do not delete the meaning:

- "Fixed a bug which would sometimes prevent players from progressing" →
  "Fixed players sometimes being unable to progress"

## Grammar rules

- Start each entry with a change word: `Added`, `Changed`, `Fixed`,
  `Removed`, `Reduced`, `Adjusted`, `Updated`.
- Keep one idea per entry. Split long entries into several bullets.
- Do not use "should", "may", "via", "e.g.", "i.e.", or "etc.".
- Do not start with filler such as "Support has been added for...". Say
  "Added ...".

## Grouping

Group entries by **scope**, not by change type. A scope is the feature,
page, screen, or area of the product the change touches — the natural
category a reader looks under when scanning for what changed. Use `###`
headings named after scopes; use `####` (or deeper) subheadings only to
break up a large scope, and the change word at the start of each bullet to
mark what kind of change it was.

Put each entry under the scope it touches. Order scopes to mirror the user's
journey through the product, or roughly by importance. When in doubt, keep
related entries together under the same scope.

### Scope style

Use clear, human-readable scope names that the product itself uses. Do not
invent opaque or internal names. Keep the names short; a scope with many
entries can carry a `####` subheading per sub-area.

Example, modelled on Wynncraft's changelog:

```
### 💎 Misc Changes
- Fixed mounts not spawning when glints are equipped
- Fixed mount models becoming malformed when returning from AFK
- Fixed being able to claim ingredient bombs while on housing build mode

### 🗺️ World & Mobs
- Changed Pernix Monkey emerald stealing behaviour
- Fixed a typo in the description of the cave The Barracks

### 🧭 Quests & Discoveries
#### King's Recruit
- Fixed the Guard becoming unresponsive under certain conditions
- Made it update your quest stage after equipping your helmet

#### Miscellaneous
- Fixed the Wrecking Ball secret discovery preventing you from leaving
```

The emoji is optional. Use it only when it aids scanning; a bare text scope
name is equally correct:

```
### Report Cards
- Added create report card on Laporan page
- Fixed PDF export cutting off the last row

### Attendance
- Changed period status button to cycle through Aktif, Selesai, and Berhenti
- Removed schedule select column from absensi add form
```

Do not use the Keep a Changelog type subheadings (`Added`, `Changed`,
`Fixed`, `Removed`, `Breaking Changes`) to organize entries. The change word
on each bullet already conveys the type; the scope heading conveys where the
change lives. Reserve `### Breaking Changes` for the rare cross-cutting
breaking change, placed first under `## [Unreleased]` when present.

## One entry per change

Record each user-visible change once per release. When a change lands in
several steps during development, do not list each step as its own entry.
Merge the steps into the single entry that describes the final shipped
behavior.

Do not write:

```
- Added dark mode
- Changed default theme to light mode
- Adjusted dark mode colors
```

Write instead:

```
- Added dark mode
```

The release body shows the finished behavior, not the development steps.
