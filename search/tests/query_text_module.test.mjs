// Runs the case file against the JavaScript module (musicindex ADR 0049 R-QUERY-3).
// The crate runs the same file. A difference between them is a defect, so correct
// the module or the crate, and do not change a case to make a test pass.
//
// Usage: node --test search/tests/query_text_module.test.mjs
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import {
  createQueryText,
  defaultRules,
  fieldTable,
  hostKeepsRow,
  hostMatches,
  indexOrderPairs,
  kindLabel,
  kindPairs,
  quoteFieldValue,
} from '../web/query-text.mjs';

const moduleUrl = new URL('../web/query-text.mjs', import.meta.url);
const file = JSON.parse(readFileSync(new URL('../cases/query-text-cases.json', import.meta.url), 'utf8'));
const {
  cases,
  cursor_cases: cursorCases,
  field_table: fieldTableCases,
  host_cases: hostCases,
  label_cases: labelCases,
  note_cases: noteCases,
  order_cases: orderCases,
  quote_cases: quoteCases,
  scope_note_cases: scopeNoteCases,
} = file;

const canonical = (value) => {
  if (Array.isArray(value)) return value.map(canonical);
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
  }
  return value;
};
const same = (a, b) => JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));

const resultOf = (queryText, text) => {
  const terms = queryText.parse(text);
  return {
    text,
    terms,
    scope: queryText.scope(terms),
    index_q: queryText.indexQ(terms),
    runnable: queryText.runnable(terms),
    plan: queryText.plan(terms),
  };
};

const failures = (queryText) => cases.filter((expected) => {
  try {
    return !same(resultOf(queryText, expected.text), expected);
  } catch {
    return true;
  }
});

test('musicindex ADR 0049 R-QUERY-3: the module gives each result of the case file', () => {
  assert.ok(cases.length > 0);
  const queryText = createQueryText();
  const differences = failures(queryText).map((expected) => [
    JSON.stringify(expected.text),
    `  case:   ${JSON.stringify(canonical(expected))}`,
    `  module: ${JSON.stringify(canonical(resultOf(queryText, expected.text)))}`,
  ].join('\n'));
  assert.equal(
    differences.length,
    0,
    `musicindex ADR 0049 R-QUERY-3: ${differences.length} of ${cases.length} cases differ. `
      + `Correct the module or the crate. Do not change a case to make this test pass.\n${differences.join('\n')}`,
  );
});

test('Stophammer ADR 0084: the module gives each order of the case file', () => {
  assert.ok(orderCases.length > 0);
  for (const { capabilities, run, sort, reverse, params } of orderCases) {
    assert.deepEqual(
      indexOrderPairs({ sort, reverse }, run, capabilities),
      params,
      `Stophammer ADR 0084: ${JSON.stringify({ capabilities, run, sort, reverse })}. Correct indexOrderPairs.`,
    );
  }
});

test('musicindex ADR 0020 R-WORD-1: the module gives each label of the case file', () => {
  assert.ok(labelCases.length > 0);
  const { releaseKind } = createQueryText();
  for (const { stated, track_count: count, label } of labelCases) {
    assert.equal(
      kindLabel(releaseKind(stated, count)),
      label,
      `musicindex ADR 0020 R-WORD-1: ${JSON.stringify({ stated, count })}. Correct releaseKind.`,
    );
  }
});

test('musicindex ADR 0052 R-QUERY-7: the module gives the field table of the case file', () => {
  assert.deepEqual(
    JSON.parse(JSON.stringify(fieldTable)),
    fieldTableCases,
    'musicindex ADR 0052 R-QUERY-7: correct fieldTable, or change the case with the ADR that owns the rule.',
  );
});

test('musicindex ADR 0058 R-QUERY-12: the module gives each quote case', () => {
  assert.ok(quoteCases.length > 0);
  for (const { text, value } of quoteCases) {
    assert.equal(
      quoteFieldValue(text),
      value,
      `musicindex ADR 0058 R-QUERY-12: quoteFieldValue(${JSON.stringify(text)}) must give ${JSON.stringify(value)}. Correct quoteFieldValue.`,
    );
  }
});

test('musicindex ADR 0058 R-QUERY-11: a placeholder is the quoted value of its example', () => {
  for (const row of fieldTable.filter((entry) => entry.placeholder !== null)) {
    assert.equal(
      row.example,
      row.token + quoteFieldValue(row.placeholder),
      `musicindex ADR 0058 R-QUERY-11: the example of ${row.token} must be the token and the quoted placeholder. Correct the row.`,
    );
  }
});

test('musicindex ADR 0052 R-QUERY-7: the module gives each cursor case', () => {
  assert.ok(cursorCases.length > 0);
  const queryText = createQueryText();
  for (const expected of cursorCases) {
    const { text, before, library } = expected;
    assert.ok(text.startsWith(before));
    const result = queryText.fieldListRows(text, before.length, library);
    const actual = {
      text,
      before,
      library,
      edit: result.edit === null
        ? null
        : { before: text.slice(0, result.edit.start), text: text.slice(result.edit.start, result.edit.end) },
      rows: result.rows,
      names: result.names,
      error: result.error,
    };
    assert.deepEqual(
      actual,
      expected,
      `musicindex ADR 0052 R-QUERY-7: ${JSON.stringify({ text, before, library })}. Correct fieldListRows.`,
    );
  }
});

test('musicindex ADR 0052 R-QUERY-8: the module gives each note case', () => {
  assert.ok(noteCases.length > 0);
  const queryText = createQueryText();
  for (const { text, library, note } of noteCases) {
    const terms = queryText.parse(text);
    assert.deepEqual(
      queryText.searchNote(terms, queryText.plan(terms), library),
      note,
      `musicindex ADR 0052 R-QUERY-8: ${JSON.stringify({ text, library })}. Correct searchNote.`,
    );
  }
});

test('musicindex ADR 0052 R-QUERY-8: the module gives each scope note case', () => {
  assert.ok(scopeNoteCases.length > 0);
  const queryText = createQueryText();
  for (const { text, entity, note } of scopeNoteCases) {
    const terms = queryText.parse(text);
    assert.deepEqual(
      queryText.scopeNote(terms, queryText.plan(terms), entity),
      note,
      `musicindex ADR 0052 R-QUERY-8: ${JSON.stringify({ text, entity })}. Correct scopeNote.`,
    );
  }
});

test('musicindex ADR 0062 R-QUERY-16: the module gives each host case', () => {
  assert.ok(hostCases.length > 0);
  for (const expected of hostCases) {
    const { value, feed_host: feedHost } = expected;
    assert.deepEqual(
      {
        value,
        feed_host: feedHost,
        matches: feedHost === null ? null : hostMatches(value, feedHost),
        host_keeps: hostKeepsRow(value, false, feedHost),
        not_host_keeps: hostKeepsRow(value, true, feedHost),
      },
      expected,
      `musicindex ADR 0062 R-QUERY-16: ${JSON.stringify({ value, feedHost })}. Correct hostMatches or hostKeepsRow.`,
    );
  }
});

// A person cannot give these values to the crate, because its type does not hold them.
test('Stophammer ADR 0084 section 5: the module sends no parameter that gives 400', () => {
  const capabilities = ['sort'];
  assert.deepEqual(indexOrderPairs({ sort: 'relevance', reverse: true }, 'command', capabilities), []);
  assert.deepEqual(indexOrderPairs({ sort: 'other', reverse: true }, 'command', capabilities), []);
});

// The usual JavaScript forms. Each one replaces one rule of the module.
const mistakes = {
  'isSpace with the regular expression \\s': { isSpace: (ch) => /\s/.test(ch) },
  'field names with toLowerCase': { lowerAscii: (text) => text.toLowerCase() },
  'isAlphanumeric with \\p{L}\\p{N}': { isAlphanumeric: (text) => /[\p{L}\p{N}]/u.test(text) },
  'a day checked with Date.UTC': {
    validDay: (year, month, day) => {
      const date = new Date(Date.UTC(year, month - 1, day));
      return date.getUTCMonth() === month - 1 && date.getUTCDate() === day && date.getUTCFullYear() === year;
    },
  },
  'a day with no check of the month length': {
    validDay: (_year, month, day) => month >= 1 && month <= 12 && day >= 1 && day <= 31,
  },
  // A value above 2^53 is also above the u32 limit, so the two forms refuse it. An
  // amount of more than 308 digits gives an exception with this form.
  'numbers with Number': {
    readNumber: (text) => (/^[0-9]+$/.test(text) ? BigInt(Number(text)) : null),
  },
};

for (const [name, rules] of Object.entries(mistakes)) {
  test(`the cases catch this JavaScript mistake: ${name}`, () => {
    const wrong = failures(createQueryText({ ...defaultRules, ...rules }));
    assert.ok(
      wrong.length > 0,
      `musicindex ADR 0049 R-QUERY-3 item 3: no case catches "${name}". Add a case that the crate passes.`,
    );
  });
}

test('musicindex ADR 0049 R-QUERY-6: each set of kinds gives its request', () => {
  const pairs = (name, value) => [['kind', 'unknown'], [name, value]];
  assert.equal(kindPairs([]), null);
  assert.equal(kindPairs(['ep']), null);
  assert.deepEqual(kindPairs(['single']), pairs('tracks', '1'));
  assert.deepEqual(kindPairs(['album']), pairs('min_tracks', '2'));
  assert.deepEqual(kindPairs(['album', 'single']), pairs('min_tracks', '1'));
  assert.deepEqual(kindPairs(['album', 'ep', 'single']), pairs('min_tracks', '1'));
  assert.deepEqual(kindPairs(['ep', 'single']), pairs('tracks', '1'));
  assert.deepEqual(kindPairs(['ep', 'album']), pairs('min_tracks', '2'));
});

// A count from 1 is in the range of a request when the label of that count is one of
// its kinds. A count of zero is the known difference of R-QUERY-6 item 1.
test('musicindex ADR 0049 R-QUERY-6: the label and the request agree', () => {
  const { releaseKind } = createQueryText();
  const all = ['album', 'ep', 'single'];
  for (let mask = 1; mask < 8; mask += 1) {
    const kinds = all.filter((_kind, bit) => mask & (1 << bit));
    const params = Object.fromEntries(kindPairs(kinds) ?? []);
    const requested = (count) => {
      if (kindPairs(kinds) === null) return false;
      if (params.tracks !== undefined) return count === Number(params.tracks);
      return count >= Number(params.min_tracks)
        && (params.max_tracks === undefined || count <= Number(params.max_tracks));
    };
    for (let count = 1; count <= 200; count += 1) {
      assert.equal(
        kinds.includes(releaseKind(null, count)),
        requested(count),
        `musicindex ADR 0049 R-QUERY-6: ${kinds} and ${count} tracks. Change TRACK_COUNTS so each set `
          + 'of kinds gives one range with no gap, or change kindPairs.',
      );
    }
  }
});

test('musicindex ADR 0049 R-QUERY-4 item 4: the module loads as one file with no dependency', () => {
  const source = readFileSync(moduleUrl, 'utf8');
  assert.doesNotMatch(
    source,
    /^\s*import\b|\bimport\(|\brequire\(|\bprocess\.|\bfetch\(|\bDeno\./m,
    'musicindex ADR 0049 R-QUERY-4 item 4: the module has no import and no I/O. The website calls fetch.',
  );
});
