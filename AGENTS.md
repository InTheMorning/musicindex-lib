# musicindex-lib

## What This Repository Is

This repository is for code that two or more apps of the suite use. These apps
are musicindex.org, musicindex-studio and HitIt. It has one folder for each
subject.

Code that only one app uses stays in that app. A player, an audio decoder and a
GPUI renderer are examples. Brand files, fonts, icons and tokens stay in
`../musicindex-assets`.

This repository states no rule. A rule about the code here lives in an ADR of the
repository that owns the subject. musicindex owns the shared interface rules.

## Where The Work Stands

`search/` holds the crate `musicindex-search`, the JavaScript module
`web/query-text.mjs` and the case file. musicindex ADR 0049 accepts `search/`. HitIt and
Studio pin the crate at commit `634e4cc`. The website keeps a copy of the module, and its
toolbar field uses that copy. The [search move plan](docs/plans/search-move-phase-plan.md)
gives the state of each phase. The crate and the module also give the field list rows and
the note words of musicindex ADR 0052. The three apps use them.

The crate and the module also read the field `host:` of musicindex ADR 0062. They give the
host match function of its rule R-QUERY-16. No app pins or copies that change at this time.

The module `name_order` of the crate is the client side of the name sort of Stophammer
ADR 0090. Stophammer keeps its own copy of the two functions. Its file
`tests/fixtures/order-key-vectors.json` is the contract, and
`search/cases/order-key-vectors.json` is a copy. The orchestrator session decided this
place on 2026-11-08. If Stophammer uses this crate, that change needs its own record.
No JavaScript port exists, because the website holds no list that merges rows by name.

No ADR accepts `moodbar/`, and it has no folder.

| Subject | Content | Research |
| --- | --- | --- |
| `search/` | The query crate for HitIt and Studio, the JavaScript module for the website, and the case file that both must pass | `research_notes/search/shared-query-text.md` in `../musicindex-research` |
| `moodbar/` | The analysis crate for Studio and HitIt, the browser drawing code, and the test data | No research document |

## How An App Uses This Code

musicindex ADR 0049 R-QUERY-4 owns these rules for `search/`:

- A Rust app uses a crate as a Git dependency, pinned to a commit.
- The website keeps a copy of each JavaScript file, because it has no build step. A
  check finds a copy with other bytes.

The orchestrator session decided this item on 2026-11-08, and no ADR states it:

- An app that merges its rows with node rows by name calls `name_order::check_vectors`
  in one of its own tests. Its toolchain and its lock file then run the vectors.

## Working Rules

These rules are advisory. The `project-baseline` skill owns them.

- Add a code folder only when an ADR accepts its subject and the code for it exists.
- Run the gate of `project-baseline` in each crate that you change.
- A crate must build with the lowest Rust version of an app that uses it.
- Write prose in ASD-STE100 Simplified Technical English.
