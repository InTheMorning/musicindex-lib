# Search Move Phase Plan

## Status

Active - 2026-11-06. This plan binds nothing. musicindex ADR 0049 owns the rules of the
query text. The research is
`research_notes/search/shared-query-text.md` in `../musicindex-research`.

## Goal

v4vmm, Studio and the website read one query text and send the same Index requests.
They also use one kind rule for the label and for the filter.

## Non-Goals

- A player, a decoder or a moodbar. A different plan gives these.
- Shared UI code. The field list stays in each app.
- An HTTP client. v4vmm keeps `reqwest`, and Studio keeps `ureq`.
- A contract crate for the Stophammer response types.
- A Library search. The Library SQL stays in v4vmm.

## Current State

The commits are from 2026-11-06. v4vmm has 15 changed files that are not in a commit,
and Studio has 9.

| Repository | Search state |
| --- | --- |
| v4vmm | `search_query.rs` has the parser. `search_run.rs` has the Index plan. `release_kind.rs` has the kind rule. It sends `kind=single`, so `kind:single` finds no Index rows. A text search found no code that sends `sort` or `reverse` on a search. |
| Studio | The pickers send text to `/musicindex/search`. Core sends `q`, `type=feed` and `limit=25`. The route holds the workspace lock during the request. |
| Website | The toolbar field sends a plain query (musicindex ADR 0047). Shell packet 002, the toolbar, is in commit `531716c`. |
| Stophammer | Release 0.18.1 is live, and its capabilities do not list `sort`. ADR 0084 is built in commits that are not deployed. ADR 0085 §6 gives the kind requests. The index has no prefix match. |

## Target State

```text
musicindex-lib/
  Cargo.toml                        workspace
  search/
    Cargo.toml                      crate musicindex-search
    src/
      query.rs                      from v4vmm search_query.rs
      plan.rs                       from v4vmm search_run.rs: the plan and the filters
      kind.rs                       from v4vmm release_kind.rs: the rule and its requests
      request.rs                    the q text and the parameters of each entity type
    cases/query-text-cases.json     the required result of each text
    web/query-text.mjs              the module for the website
    tests/                          the crate and the module each read the case file
```

These parts stay in the apps:

| App | Part |
| --- | --- |
| v4vmm | `with_library_scope`, `library_scope_of`, the Library queries, the run generations, the field list and the page rows |
| Studio | The route, the workspace lock, the picker UI and the `ureq` request |
| Website | The toolbar, the result rows and `fetch` |

The crate has no I/O, no GPUI type and no HTTP type. It gives the path parameters, and the app sends them.
It builds with Rust 1.93, because Studio has that minimum. v4vmm has 1.97.

## Phases

### Phase 0: Decisions

1. A v4vmm ADR amends ADR 0087, because "One parser in the view model layer" changes.
2. A Studio ADR states which search fields read the syntax.

No code moves in this phase.

### Phase 1: The Crate

Move the code with no change of behavior. Keep the source commit in the commit message.

- Mechanical: the crate passes each parser test from v4vmm.
- Mechanical: the crate passes `query-text-cases.json`, which comes from the capture of 2026-11-06.
- Mechanical: the crate builds with Rust 1.93 in CI.

### Phase 2: v4vmm Uses The Crate

Start after the search and sort work that is in progress is in a commit. v4vmm is the first user, because its tests already cover this code.

- `search_query.rs` keeps the two scope functions and gives the other names from the crate, so the other files do not change.
- These guards change in the same commit: `ADR_0087_QUERY_PARSER_OWNER`, `RUN_OWNER` and the `release_kind.rs` guards in `tests/architecture_tests.rs`.
- Mechanical: each v4vmm search test passes, with no change to the test.

### Phase 3: Two Behavior Corrections

Each correction changes the case file in the same commit. Stophammer changes in neither one.

1. **Kind.** The crate sends the requests of Stophammer ADR 0085 §6. `kind:ep` sends no Index request.
2. **Quotes.** The crate writes a space for each quote and each backslash in `q` and in a text filter.

- Mechanical: a case gives `kind=unknown&tracks=1` for `kind:single`.
- Mechanical: the round trip test of `tools/query-text` in the research repository gives no difference for `q`.
- Visual: in v4vmm, `kind:single` shows Index rows, and each row has the label "Single".

### Phase 4: Studio Pickers

1. The search route sends its request with no lock of the workspace, as `nostr_profiles` does.
2. The core search takes the output of the crate. A field error goes to the UI as a message.
3. A decode error gives an error, not an empty list.

- Mechanical: a test holds the workspace lock and a search still completes.
- Mechanical: a test gives a `kind:single` text to the route, and the fake index receives `kind=unknown&tracks=1`.
- Visual: the recipient picker, with a result and with a field error.

This phase can start after Phase 1, if Studio needs it before v4vmm is ready.

### Phase 5: Website Module

1. Move the prototype to `search/web/query-text.mjs`, with the two corrections of Phase 3.
2. The website keeps a copy of the module and the case file. `tools/check-site.sh` finds a copy with other bytes.
3. The toolbar field sends the output of the module.

- Mechanical: the module passes the case file in the CI of musicindex-lib, in Node.
- Mechanical: `tools/check-site.sh` fails when a copy has other bytes.
- Visual: in Firefox and Chromium, a search with fields, and the note for a Library field.

The field list and `cursor_term` are not in this phase. The case file has no cursor cases.

### Phase 6: Sort And Prefix

Start when Stophammer deploys each one. The crate follows the deployed contract, not a contract in work.

- Add `sort` and `reverse` to the request when a node states the capability `sort` (Stophammer ADR 0084 §11).
- When Stophammer accepts a prefix match, the crate needs no change unless the contract gives a new parameter.

## Risks

| Risk | Control |
| --- | --- |
| The v4vmm working tree changes during the move | Phase 2 starts after the work is in a commit |
| The Stophammer contract changes | The crate uses only deployed parameters, and a case holds each parameter name |
| A feed states a kind | Stophammer ADR 0085 §6 states that the mapping must then change, and `kind=unknown` does not find such a feed |
| `episode_count` differs from the true track count | The label and the filter read the same field. The research did not compare it with the track rows. |
| A browser differs from Node | The visual check of Phase 5 |

## Test Strategy

One case file is the shared test. The crate and the module each run it in the CI of this repository.
Each change of a case needs the ADR that owns the rule.

## Rollback

- v4vmm and Studio pin one commit, so a return to the previous commit restores the previous crate.
- v4vmm can restore its local modules with a revert of the Phase 2 commit.
- The website can restore a plain query with a revert of the toolbar change.

## Open Questions

- Which settings give an inferred EP? The operator will test settings after the crate exists. `kind:ep` has no inference until then.
- Which Studio search fields read the syntax?
