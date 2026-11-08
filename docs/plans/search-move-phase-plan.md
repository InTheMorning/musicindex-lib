# Search Move Phase Plan

## Status

Active - 2026-11-07. This plan binds nothing. musicindex ADR 0049 owns the rules of the
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

These facts come from the repositories and the live node on 2026-11-07.

| Repository | Search state |
| --- | --- |
| v4vmm | It pins the crate at commit `7cfc4e7` (v4vmm ADR 0091, steps 1 and 2). `search_query.rs` keeps the two scope functions. |
| Studio | It pins the crate at commit `7cfc4e7` (Studio ADR 0151). `studio-core` reads each typed search text with the crate. |
| Website | The toolbar field sends the output of the module (musicindex commit `465b9af`). The live page loads the module. `query/` holds copies of the module and the case file, and they have the same bytes as the files here. |
| Stophammer | The capabilities of the live node list `sort` (ADR 0084 §11). ADR 0085 §6 gives the kind requests. The live node has the prefix match of ADR 0086. |

| Phase | State |
| --- | --- |
| 0 | Complete: v4vmm ADR 0091 and Studio ADR 0151 |
| 1, 1a and 3a | Complete |
| 2 | Complete: v4vmm commits `bebfe97` and `c433582` |
| 3 | Complete. The operator passed the visual check, as v4vmm commit `9aed106` records (v4vmm ADR 0087, Operator Answers item 8). |
| 4 | Complete: Studio commit `2ecd7e9`. Studio records that the visual checks passed. |
| 5 | Complete. The operator passed the two visual gates of musicindex ADR 0049, as musicindex commit `6b8e824` records. |
| 6 | Complete, with no change to the crate |

The commits of this repository after `7cfc4e7` change no behavior of the crate. `QueryTerm::can_run` is the one new function, and only an app that uses it needs a new pin. v4vmm still has its own copy of that test in `query_can_run`.

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

### Phase 1a: A Cause For A Query With Only Exclusions

On 2026-11-07 the operator asked for a skip value of its own, because v4vmm will show a note for this cause. The value is `IndexSkip::ExclusionsOnly`. The Index request does not change, because the crate sends no request for this query. No ADR states this value.

- Mechanical: a case gives `exclusions_only` for `-remix`.
- Mechanical: a case gives `no_index_part` for `-remix kind:single kind:ep`, because its `kind:` terms share no kind.

### Phase 2: v4vmm Uses The Crate

Start after the search and sort work that is in progress is in a commit. v4vmm is the first user, because its tests already cover this code.

- `search_query.rs` keeps the two scope functions and gives the other names from the crate, so the other files do not change.
- These guards change in the same commit: `ADR_0087_QUERY_PARSER_OWNER`, `RUN_OWNER` and the `release_kind.rs` guards in `tests/architecture_tests.rs`.
- Mechanical: each v4vmm search test passes, with no change to the test, on the commit of Phase 1. A pin after Phase 1a adds `IndexSkip::ExclusionsOnly` to two `match` expressions in `src/view_models/search_results/mod.rs`.

### Phase 3: Two Behavior Corrections

Each correction changes the case file in the same commit. Stophammer changes in neither one.

1. **Kind.** The crate sends the requests of Stophammer ADR 0085 §6. `kind:ep` sends no Index request.
2. **Quotes.** The crate writes a space for each quote and each backslash in `q` and in a text filter.

- Mechanical: a case gives `kind=unknown&tracks=1` for `kind:single`.
- Mechanical: the round trip test of `tools/query-text` in the research repository gives no difference for `q`.
- Visual, passed by the operator from agent screenshots: in v4vmm, `kind:single` shows Index rows, and each row has the label "Single".

### Phase 3a: Sort And Reverse

The crate gives `sort` and `reverse`, because the live node lists `sort` in its capabilities. Stophammer ADR 0084 owns these parameters.

1. A node that does not list `sort` gets no `sort` and no `reverse` (Stophammer ADR 0084 §11).
2. A live search keeps `relevance`, and it sends no `sort` and no `reverse` (Stophammer ADR 0084, Consequences).
3. `relevance` sends no `sort` and no `reverse`, so the crate cannot cause `relevance_needs_word` or `relevance_cannot_reverse` (Stophammer ADR 0084 §4 and §5).
4. The case file holds each result, so the module of Phase 5 passes the same tests.

- Mechanical: the case file gives no parameter for a node with no `sort`.
- Mechanical: the case file gives no parameter for a live search.
- Mechanical: the case file gives `sort=date&reverse=true` when a person selects Oldest.

### Phase 4: Studio Pickers

1. The search route sends its request with no lock of the workspace, as `nostr_profiles` does.
2. The core search takes the output of the crate. A field error goes to the UI as a message.
3. A decode error gives an error, not an empty list.

- Mechanical: a test holds the workspace lock and a search still completes.
- Mechanical: a test gives a `kind:single` text to the route, and the fake index receives `kind=unknown&tracks=1`.
- Visual: the recipient picker, with a result and with a field error.

This phase can start after Phase 1, if Studio needs it before v4vmm is ready.

### Phase 5: Website Module

1. Move the prototype to `search/web/query-text.mjs`, with the skip value of Phase 1a, the two corrections of Phase 3 and the parameters of Phase 3a.
2. The website keeps a copy of the module and the case file. `tools/check-site.sh` finds a copy with other bytes.
3. The toolbar field sends the output of the module.

- Mechanical: the module passes the case file in the CI of musicindex-lib, in Node.
- Mechanical: the cases catch each common JavaScript mistake of `tools/query-text/compare.mjs`.
- Mechanical: the crate and the module pass the label cases of the case file.
- Mechanical: `tools/check-site.sh` fails when a copy has other bytes.
- Visual, passed by the operator: a search with fields in Firefox and Chromium (musicindex ADR 0049 R-QUERY-4 item 4).
- Visual, passed by the operator: the note for a Library field, in Light and Dark (musicindex ADR 0049 R-QUERY-2).

The operator looked on the live site at musicindex commit `465b9af`, in Firefox and Chromium windows. The orchestrator session relayed the pass. No real phone and no browser with WebKit ran this change, and musicindex ADR 0049 does not require them. The website review is `docs/reviews/search-move-phase-5-website-review.md` in `../musicindex`. It also records that a search row on the website shows no type label, so `kind:single` does not show the label "Single" there.

The field list and `cursor_term` are not in this phase. The case file has no cursor cases.

A browser field can hold a text with a lone surrogate, and a Rust string cannot. Thus such a text has no crate result, and its result in the module is unproved. This note states no rule.

The apps differ on `-!!!` and `--`. The plan of each text is `exclusions_only`, because the text has no positive term. v4vmm runs nothing for such a text, so it shows no note (v4vmm ADR 0087 Decision 4). Studio and the website show their notes. This note states no rule, and no change is necessary now.

### Phase 6: Prefix

Stophammer ADR 0086 gives the prefix match. It adds no parameter, so the crate and the module need no change. The node reads the last word of `q` as a prefix.

- Mechanical: on 2026-11-07, the live node gave rows for `q=jsmo`, and no rows for `q="jsmo"` or `artist=jsmo`.

## Risks

| Risk | Control |
| --- | --- |
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
