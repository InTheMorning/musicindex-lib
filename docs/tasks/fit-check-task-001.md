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

## Amendment Of 2026-11-13: Two Faults On The Website

The orchestrator session sent this change on 2026-11-13. The website session adds the two faults to R-FIT-3 in an
amendment of musicindex ADR 0065. The website showed the two faults at 761 px.

- `overflows` also gives each visible control with content that is wider than the control, when the control does not
  clip its content. The finding has `box: 'control'`. On the website, the segment "Releases 20+" had content of 94 px in a
  box of 82 px, and the count went into the segment "Tracks".
- The new helper `overlaps` gives each pair of visible controls with boxes that intersect by more than 1 px. A disabled
  control counts. A control under an open dialog or popover does not count. On the website, Back and Forward were on the
  search field from 761 px to about 1100 px.
- `fitMessage` gives a fix for the two faults.

Mechanical, proved by `fit/tests/fit_check.test.mjs`:

6. On a page with one segment that is too narrow, `overflows` returns one finding with `box: 'control'`, and the other
   helpers return none.
7. On a page with a disabled button on a text field, `overlaps` returns one finding, and the other helpers return none.
8. On the same page with an open modal dialog, `overlaps` returns no finding.
9. The correct page has segments, buttons and a field that do not overlap, and each helper returns no finding.

## Amendment Of 2026-11-13: A Control In The Padding Of A Field

The website session amends R-FIT-3: a control that is fully in the padding of a text field is not an overlap. The
website walk found the Clear button of `input#q` in the right padding of 44 px, and the text never goes under it.

- For an input or a textarea, `overlaps` uses the content box: the border box without the border and the padding.
- A control on the content box of a field is still an overlap.

Mechanical, proved by `fit/tests/fit_check.test.mjs`:

10. On a page with a button of 32 px in the right padding of 44 px of a field, each helper returns no finding.
11. The overlap page still gives one finding.
