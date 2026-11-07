# musicindex-lib

## What This Repository Is

This repository is for code that two or more apps of the suite use. These apps
are musicindex.org, musicindex-studio and v4vmm. It has one folder for each
subject.

Code that only one app uses stays in that app. A player, an audio decoder and a
GPUI renderer are examples. Brand files, fonts, icons and tokens stay in
`../musicindex-assets`.

This repository states no rule. A rule about the code here lives in an ADR of the
repository that owns the subject. musicindex owns the shared interface rules.

## Where The Work Stands

`search/` holds the crate `musicindex-search`, the JavaScript module
`web/query-text.mjs` and the case file. musicindex ADR 0049 accepts `search/`. v4vmm and
Studio pin the crate at commit `7cfc4e7`. The website keeps a copy of the module, and its
toolbar field uses that copy. The [search move plan](docs/plans/search-move-phase-plan.md)
gives the state of each phase.
No ADR accepts `moodbar/`, and it has no folder.

| Subject | Content | Research |
| --- | --- | --- |
| `search/` | The query crate for v4vmm and Studio, the JavaScript module for the website, and the case file that both must pass | `research_notes/search/shared-query-text.md` in `../musicindex-research` |
| `moodbar/` | The analysis crate for Studio and v4vmm, the browser drawing code, and the test data | No research document |

## How An App Uses This Code

musicindex ADR 0049 R-QUERY-4 owns these rules for `search/`:

- A Rust app uses a crate as a Git dependency, pinned to a commit.
- The website keeps a copy of each JavaScript file, because it has no build step. A
  check finds a copy with other bytes.

## Working Rules

These rules are advisory. The `project-baseline` skill owns them.

- Add a code folder only when an ADR accepts its subject and the code for it exists.
- Run the gate of `project-baseline` in each crate that you change.
- A crate must build with the lowest Rust version of an app that uses it.
- Write prose in ASD-STE100 Simplified Technical English.
