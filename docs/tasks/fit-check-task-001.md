# Task 001: The Shared Browser Fit Check

Owner: musicindex ADR 0065 R-FIT-3 and R-FIT-5. The operator accepted ADR 0065 on 2026-11-13.
The orchestrator session sent this task on 2026-11-13.

## Goal

Add one browser module with three helpers to musicindex-lib. The website and Studio use a copy of it in their walks.

- `edgeCuts` finds a text or a control that an edge of a group cuts or almost touches.
- `overflows` finds an element that goes further than the right edge of the window.
- `wordBreaks` finds a line of text that ends between two letters of one word.

Each helper runs in the page and returns a list of findings. A helper does not exit.

## Selections Of This Session

The ADR leaves these two items to this session:

- The module path is `fit/web/fit-check.mjs`. The subject is the fit of a layout, not a search. Thus it gets its own
  folder adjacent to `search/`, with the same `web/` and `tests/` layout.
- The opt-out attribute is `data-fit-allow-break`. The word check does not count a break in an element that has this
  attribute, or in its descendants. The two web apps use this name.

## Files

- Read: `../musicindex/docs/adr/0065-each-app-checks-its-fit-at-two-widths.md`,
  `../musicindex-studio/tools/drive-ui.mjs` (lines 88 to 150).
- Change: `fit/web/fit-check.mjs` (new), `fit/tests/fit_check.test.mjs` (new), `AGENTS.md`.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Edge cut check | extend | ../musicindex-studio/tools/drive-ui.mjs | Moved from `$edges`. Studio removes its copy (ADR 0065 R-FIT-5 item 4). The selector, the inset and the disclosure step become options. |
| Overflow check | extend | ../musicindex-studio/tools/drive-ui.mjs | Moved from `$overflow`. Studio removes its copy. Each finding becomes an object. |
| Word break check | new | | Searched ../musicindex-studio/tools, ../musicindex/tools, ../musicindex-lib/search/web: no check for a break inside a word. |
| Headless browser driver for the test | extend | ../musicindex-studio/tools/drive-ui.mjs | The test uses the same DevTools protocol calls in a smaller form. |
| Shared browser module layout | reuse | search/web/query-text.mjs | Same layout: one file, no build step, no dependency. |

## Acceptance Criteria

Mechanical, proved by `fit/tests/fit_check.test.mjs`:

1. On a page with one edge cut, `edgeCuts` returns one finding, and the other two helpers return none.
2. On a page with one overflow, `overflows` returns one finding, and the other two helpers return none.
3. On a page with one break inside a word, `wordBreaks` returns one finding, and the other two helpers return none.
4. On a correct page, each helper returns no finding. That page has a long URL in an element with
   `data-fit-allow-break`, and a wide table in a scroll area.
5. `fitMessage` gives a text that names musicindex ADR 0065 R-FIT-3, the width and the fix.

Visual: none. The apps own the visual checks of ADR 0065.
