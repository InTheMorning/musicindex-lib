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

The repository has no code. Two subjects are proposed, and no ADR accepts them:

| Subject | Proposed content | Research |
| --- | --- | --- |
| `search/` | The query text parser for the website and Studio, and the case file that each parser must pass | `research_notes/search/shared-query-text.md` in `../musicindex-research` |
| `moodbar/` | The analysis crate for Studio and v4vmm, the browser drawing code, and the test data | No research document |

## How An App Uses This Code

These are proposals until an ADR accepts them:

- A Rust app uses a crate as a Git dependency, pinned to a commit.
- The website and Studio keep a copy of each JavaScript file, because they have no
  build step. A check finds a copy with other bytes.

## Working Rules

These rules are advisory. The `project-baseline` skill owns them.

- Add a folder only when an ADR accepts its subject and the code for it exists.
- Run the gate of `project-baseline` in each crate that you change.
- A crate must build with the lowest Rust version of an app that uses it.
- Write prose in ASD-STE100 Simplified Technical English.
