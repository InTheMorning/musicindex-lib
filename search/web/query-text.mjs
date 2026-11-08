// The query text of the Music Index apps, for the website (musicindex ADR 0049).
//
// The crate musicindex-search is the same code in Rust, and the two pass
// search/cases/query-text-cases.json. A difference between them is a defect.
// The module runs in a browser with no build step and no dependency. It has no
// I/O. The website sends the requests with its own fetch code.
//
// createQueryText takes other rules, so a test can replace one rule at a time
// and find what the cases catch. The website uses the default rules.

const WHITE_SPACE = /[\t-\r \u0085\u00A0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000]/;
const ALPHANUMERIC = /[\p{Alphabetic}\p{N}]/u;
const U32_MAX = 4294967295n;
const U64_MAX = 18446744073709551615n;

export const defaultRules = {
  isSpace: (ch) => WHITE_SPACE.test(ch),
  lowerAscii: (text) => text.replace(/[A-Z]/g, (ch) => ch.toLowerCase()),
  isAlphanumeric: (text) => ALPHANUMERIC.test(text),
  validDay: (year, month, day) => {
    if (month < 1 || month > 12 || day < 1) return false;
    const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
    return day <= [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month - 1];
  },
  readNumber: (text) => {
    if (!/^[0-9]+$/.test(text)) return null;
    const value = BigInt(text);
    return value <= U64_MAX ? value : null;
  },
};

const fixed = (...pairs) => pairs.map(([word, description]) => ({ word, description }));

// The field table, in the order of v4vmm ADR 0087 Decision 2 (musicindex ADR 0052
// R-QUERY-7). The crate holds the same table.
export const fieldTable = Object.freeze([
  { token: 'artist:', value_kind: 'text', values: 'an artist name', example: 'artist:jsmovie', description: 'The track artist or the album artist', library_only: false, fixed_values: [] },
  { token: 'title:', value_kind: 'text', values: 'a title', example: 'title:"night drive"', description: 'The track title or the release title', library_only: false, fixed_values: [] },
  { token: 'release:', value_kind: 'text', values: 'a release title', example: 'release:"aged friends"', description: 'The release title', library_only: false, fixed_values: [] },
  {
    token: 'kind:', value_kind: 'kind_list', values: 'album, ep, single', example: 'kind:single', description: 'The release kind. Use commas for more than one kind', library_only: false,
    fixed_values: fixed(['album', 'A release with many tracks'], ['ep', 'A short release'], ['single', 'A release with one track']),
  },
  {
    token: 'in:', value_kind: 'scope', values: 'library, index', example: 'in:library', description: 'Search only the Library or only the Index', library_only: false,
    fixed_values: fixed(['library', 'Only the Library'], ['index', 'Only Music Index']),
  },
  { token: 'after:', value_kind: 'date', values: 'a date or a month', example: 'after:2026-10-01', description: 'Released on this date or after it', library_only: false, fixed_values: [] },
  { token: 'before:', value_kind: 'date', values: 'a date or a month', example: 'before:2026-10', description: 'Released on this date or before it', library_only: false, fixed_values: [] },
  { token: 'duration:', value_kind: 'duration', values: '>5m, <3:30, 3m..5m', example: 'duration:3m..5m', description: 'The track duration', library_only: false, fixed_values: [] },
  {
    token: 'is:', value_kind: 'library_state', values: 'downloaded, partial, ready, removed', example: 'is:downloaded', description: 'The Library state of the track', library_only: true,
    fixed_values: fixed(['downloaded', 'Each track is downloaded'], ['partial', 'Some tracks are downloaded'], ['ready', 'Ready for a show'], ['removed', 'Removed from its feed']),
  },
  { token: 'playlist:', value_kind: 'text', values: 'a playlist name', example: 'playlist:"friday show"', description: 'The tracks of a playlist', library_only: true, fixed_values: [] },
  {
    token: 'has:', value_kind: 'content', values: 'split, image', example: 'has:split', description: 'A value split or an image of its own', library_only: true,
    fixed_values: fixed(['split', 'A value block with payment routes'], ['image', 'An image of its own']),
  },
].map((row) => Object.freeze({ ...row, fixed_values: Object.freeze(row.fixed_values.map(Object.freeze)) })));

// The rows of the field table by the field name without its colon.
const FIELDS = Object.fromEntries(fieldTable.map((row) => [row.token.slice(0, -1), row]));

const wordsOf = (name) => FIELDS[name].fixed_values.map((value) => value.word);

// The fields whose value rows are names, which the app gives.
const NAME_FIELDS = ['artist', 'release', 'playlist'];

// `true` when the field list of an app shows the rows of the field `name`
// (musicindex ADR 0052 R-QUERY-7 item 3). `library` is `present` or `absent`.
// With no Library, `in:` has no row, because `in:index` alone changes nothing.
const showsField = (library, name) => {
  if (library === 'present') return true;
  if (library === 'absent') return !FIELDS[name].library_only && name !== 'in';
  throw new TypeError('library is "present" or "absent"');
};

// Joins the tokens: `a`, `a and b`, or `a, b and c`.
const tokenList = (tokens) => (tokens.length < 2
  ? tokens.join('')
  : `${tokens.slice(0, -1).join(', ')} and ${tokens[tokens.length - 1]}`);

const note = (title, text) => ({ title, text });
const NEEDS_A_WORD = note('Music Index needs a word or a field to search for', 'Add a word, a phrase or a field.');

// The count rule: the track counts that give each kind when the feed states no
// kind (musicindex ADR 0049 R-QUERY-6). Null means that no count gives the kind,
// so `kind:` sends no Index request for it. The label and the request both read
// this table, and a test of EP settings changes this table only.
const TRACK_COUNTS = {
  album: { min: 2, max: null },
  ep: null,
  single: { min: 1, max: 1 },
};

// The value of `kind` that finds a feed with no stated kind (Stophammer ADR 0085
// section 6).
const UNSTATED_KIND = 'unknown';

const KIND_LABELS = { album: 'Album', ep: 'EP', single: 'Single' };

// The visible label of a kind that releaseKind gives.
export const kindLabel = (kind) => KIND_LABELS[kind];

const inCounts = (counts, count) => counts !== null
  && count >= counts.min
  && (counts.max === null || count <= counts.max);

// The Index parameters of the `kind:` values `kinds`, or null when the values send
// no Index request. The result holds each track count from the lowest limit to the
// highest limit, so the count rule must give a range with no gap for each set of
// values.
export const kindPairs = (kinds) => {
  const ranges = kinds.map((kind) => TRACK_COUNTS[kind]).filter((counts) => counts !== null);
  if (ranges.length === 0) return null;
  const min = Math.min(...ranges.map((counts) => counts.min));
  const max = ranges.some((counts) => counts.max === null)
    ? null
    : Math.max(...ranges.map((counts) => counts.max));
  const pairs = [['kind', UNSTATED_KIND]];
  if (max === min) {
    pairs.push(['tracks', String(min)]);
  } else {
    pairs.push(['min_tracks', String(min)]);
    if (max !== null) pairs.push(['max_tracks', String(max)]);
  }
  return pairs;
};

// The `sort` and `reverse` parameters of one search (Stophammer ADR 0084).
//
// `order` is { sort, reverse }, with the sort `relevance`, `name` or `date`. `run`
// is `live` or `command`. `capabilities` is the list `capabilities` of
// GET /v1/node/capabilities. A node that does not list `sort` gets no parameter
// (section 11). A live search keeps `relevance` (Consequences). `relevance` gets no
// parameter, because a request with no word gives 400 for `sort=relevance` (section
// 5), and the default of the node is the same sequence (section 4). A cursor
// continues only the sequence that gave it (section 9), so the website keeps one
// order for all pages of a run.
export const indexOrderPairs = (order, run, capabilities) => {
  if (!capabilities.includes('sort') || run === 'live') return [];
  if (order.sort !== 'name' && order.sort !== 'date') return [];
  const pairs = [['sort', order.sort]];
  if (order.reverse === true) pairs.push(['reverse', 'true']);
  return pairs;
};

const pad = (number, width) => String(number).padStart(width, '0');
const dayText = (year, month, day) => `${pad(year, 4)}-${pad(month, 2)}-${pad(day, 2)}`;

export function createQueryText(overrides = {}) {
  const rules = { ...defaultRules, ...overrides };

  const trim = (text) => {
    let start = 0;
    let end = text.length;
    while (start < end && rules.isSpace(text[start])) start += 1;
    while (end > start && rules.isSpace(text[end - 1])) end -= 1;
    return text.slice(start, end);
  };

  const fieldOf = (name) => {
    const lower = rules.lowerAscii(name);
    return Object.hasOwn(FIELDS, lower) ? lower : null;
  };

  const hasValuePrefix = (name, prefix) => {
    const lower = prefix.toLowerCase();
    return wordsOf(name).some((word) => word.startsWith(lower));
  };

  const readQuoted = (text, quoteAt) => {
    let content = '';
    let at = quoteAt + 1;
    while (at < text.length) {
      const ch = text[at];
      if (ch === '"') return { content, end: at + 1 };
      if (ch === '\\') {
        const next = text[at + 1];
        if (next === '"' || next === '\\') {
          content += next;
          at += 2;
        } else if (next === undefined) {
          content += '\\';
          at += 1;
        } else {
          content += `\\${next}`;
          at += 2;
        }
        continue;
      }
      content += ch;
      at += 1;
    }
    return { content, end: text.length };
  };

  const readDate = (value) => {
    if (!/^[\x00-\x7F]*$/.test(value) || value[4] !== '-') return null;
    const year = rules.readNumber(value.slice(0, 4));
    if (year === null) return null;
    const y = Number(year);
    if (value.length === 10 && value[7] === '-') {
      const month = rules.readNumber(value.slice(5, 7));
      const day = rules.readNumber(value.slice(8, 10));
      if (month === null || day === null) return null;
      if (!rules.validDay(y, Number(month), Number(day))) return null;
      const text = dayText(y, Number(month), Number(day));
      return { first_day: text, last_day: text };
    }
    if (value.length === 7) {
      const month = rules.readNumber(value.slice(5, 7));
      if (month === null || !rules.validDay(y, Number(month), 1)) return null;
      let last = 31;
      while (!rules.validDay(y, Number(month), last)) last -= 1;
      return { first_day: dayText(y, Number(month), 1), last_day: dayText(y, Number(month), last) };
    }
    return null;
  };

  const checked = (value) => (value <= U64_MAX ? value : null);

  const readSexagesimal = (text) => {
    if (text.length !== 2) return null;
    const value = rules.readNumber(text);
    return value !== null && value < 60n ? value : null;
  };

  const readClock = (text) => {
    const parts = text.split(':');
    if (parts.length === 2) {
      const minutes = rules.readNumber(parts[0]);
      const seconds = readSexagesimal(parts[1]);
      if (minutes === null || seconds === null) return null;
      const scaled = checked(minutes * 60n);
      return scaled === null ? null : checked(scaled + seconds);
    }
    if (parts.length === 3) {
      const hours = rules.readNumber(parts[0]);
      const minutes = readSexagesimal(parts[1]);
      const seconds = readSexagesimal(parts[2]);
      if (hours === null || minutes === null || seconds === null) return null;
      const scaled = checked(hours * 3600n);
      if (scaled === null) return null;
      const withMinutes = checked(scaled + minutes * 60n);
      return withMinutes === null ? null : checked(withMinutes + seconds);
    }
    return null;
  };

  const readUnits = (text) => {
    if (text === '') return null;
    const factors = { h: [1, 3600n], m: [2, 60n], s: [3, 1n] };
    let total = 0n;
    let lastRank = 0;
    let rest = text;
    while (rest !== '') {
      const digits = /^[0-9]*/.exec(rest)[0];
      const amount = rules.readNumber(digits);
      if (amount === null) return null;
      const unit = rest[digits.length];
      if (unit === undefined) return null;
      const entry = factors[rules.lowerAscii(unit)];
      if (entry === undefined || !Object.hasOwn(factors, rules.lowerAscii(unit))) return null;
      const [rank, factor] = entry;
      if (rank <= lastRank) return null;
      lastRank = rank;
      const scaled = checked(amount * factor);
      if (scaled === null) return null;
      total = checked(total + scaled);
      if (total === null) return null;
      rest = rest.slice(digits.length + 1);
    }
    return total;
  };

  const readAmount = (text) => {
    const secs = text.includes(':') ? readClock(text) : readUnits(text);
    return secs !== null && secs <= U32_MAX ? secs : null;
  };

  const readDuration = (value) => {
    const range = value.indexOf('..');
    if (range >= 0) {
      const min = readAmount(value.slice(0, range));
      const max = readAmount(value.slice(range + 2));
      if (min === null || max === null || min > max) return null;
      return { min_secs: Number(min), max_secs: Number(max) };
    }
    if (value.startsWith('>=')) {
      const min = readAmount(value.slice(2));
      return min === null ? null : { min_secs: Number(min), max_secs: null };
    }
    if (value.startsWith('>')) {
      const min = readAmount(value.slice(1));
      if (min === null || min === U32_MAX) return null;
      return { min_secs: Number(min) + 1, max_secs: null };
    }
    if (value.startsWith('<=')) {
      const max = readAmount(value.slice(2));
      return max === null ? null : { min_secs: null, max_secs: Number(max) };
    }
    if (value.startsWith('<')) {
      const max = readAmount(value.slice(1));
      if (max === null || max === 0n) return null;
      return { min_secs: null, max_secs: Number(max) - 1 };
    }
    return null;
  };

  const unknownValue = (name, value) => ({
    code: 'unknown_value',
    message: `${name}: cannot read ${value}. Use one of these values: ${FIELDS[name].values}`,
  });

  const kindsState = (value) => {
    const items = value.split(',').map(trim).filter((item) => item !== '');
    const kinds = [];
    for (const [index, item] of items.entries()) {
      const kind = wordsOf('kind').find((word) => rules.lowerAscii(item) === word);
      if (kind === undefined) {
        if (index + 1 === items.length && hasValuePrefix('kind', item)) break;
        return { state: 'invalid', error: unknownValue('kind', item) };
      }
      if (!kinds.includes(kind)) kinds.push(kind);
    }
    return kinds.length === 0 ? { state: 'incomplete' } : { state: 'valid', value: { kinds } };
  };

  const oneValueState = (name, value, key) => {
    if (value.includes(',')) {
      return {
        state: 'invalid',
        error: { code: 'one_value_only', message: `${name}: takes one value, for example ${FIELDS[name].example}` },
      };
    }
    const trimmed = trim(value);
    const word = wordsOf(name).find((candidate) => rules.lowerAscii(trimmed) === candidate);
    if (word !== undefined) return { state: 'valid', value: { [key]: word } };
    if (hasValuePrefix(name, trimmed)) return { state: 'incomplete' };
    return { state: 'invalid', error: unknownValue(name, value) };
  };

  const exampleValue = (name) => FIELDS[name].example.slice(name.length + 1);

  const valueState = (name, value, excluded) => {
    if (trim(value) === '') return { state: 'incomplete' };
    switch (FIELDS[name].value_kind) {
      case 'text':
        return { state: 'valid', value: { text: value } };
      case 'kind_list':
        return kindsState(value);
      case 'scope':
        if (excluded) {
          return {
            state: 'invalid',
            error: { code: 'excluded_scope', message: 'in: cannot have a -. Use in:library or in:index' },
          };
        }
        return oneValueState(name, value, 'scope');
      case 'date': {
        const date = readDate(value);
        return date === null
          ? { state: 'invalid', error: { code: 'not_a_date', message: `${name}: needs a date, for example ${exampleValue(name)}` } }
          : { state: 'valid', value: date };
      }
      case 'duration': {
        const range = readDuration(value);
        return range === null
          ? { state: 'invalid', error: { code: 'not_a_duration', message: `duration: needs a comparison or a range, for example ${exampleValue('duration')}` } }
          : { state: 'valid', value: range };
      }
      case 'library_state':
        return oneValueState(name, value, 'state');
      default:
        return oneValueState(name, value, 'content');
    }
  };

  const fieldTerm = (raw, excluded, name, value) => {
    const { state, value: typed = null, error = null } = valueState(name, value, excluded);
    return { raw, excluded, kind: 'field', field: `${name}:`, state, value: typed, error };
  };

  const parse = (text) => {
    const terms = [];
    let pos = 0;
    while (pos < text.length) {
      if (rules.isSpace(text[pos])) {
        pos += 1;
        continue;
      }
      const excluded = text[pos] === '-';
      const start = excluded ? pos + 1 : pos;
      if (start >= text.length || rules.isSpace(text[start])) {
        pos = start;
        continue;
      }
      if (text[start] === '"') {
        const quoted = readQuoted(text, start);
        if (quoted.content !== '') {
          terms.push({ raw: text.slice(pos, quoted.end), excluded, kind: 'phrase', text: quoted.content });
        }
        pos = quoted.end;
        continue;
      }
      let wordEnd = start;
      while (wordEnd < text.length && !rules.isSpace(text[wordEnd])) wordEnd += 1;
      const word = text.slice(start, wordEnd);
      const colon = word.indexOf(':');
      const name = colon >= 0 ? fieldOf(word.slice(0, colon)) : null;
      if (name !== null) {
        const valueStart = start + colon + 1;
        if (text[valueStart] === '"') {
          const quoted = readQuoted(text, valueStart);
          terms.push(fieldTerm(text.slice(pos, quoted.end), excluded, name, quoted.content));
          pos = quoted.end;
        } else {
          terms.push(fieldTerm(text.slice(pos, wordEnd), excluded, name, text.slice(valueStart, wordEnd)));
          pos = wordEnd;
        }
        continue;
      }
      terms.push({ raw: text.slice(pos, wordEnd), excluded, kind: 'word', text: word });
      pos = wordEnd;
    }
    return terms;
  };

  const validFields = (terms) => terms.filter((term) => term.kind === 'field' && term.state === 'valid');
  const fieldName = (term) => term.field.slice(0, -1);

  const scope = (terms) => {
    let libraryField = false;
    let inLibrary = false;
    let inIndex = false;
    for (const term of validFields(terms)) {
      if (FIELDS[fieldName(term)].library_only) libraryField = true;
      if (term.value.scope === 'library') inLibrary = true;
      if (term.value.scope === 'index') inIndex = true;
    }
    if (inIndex && (libraryField || inLibrary)) return 'conflict';
    if (inIndex) return 'index_only';
    return libraryField || inLibrary ? 'library_only' : 'everything';
  };

  // A space for each quote and each backslash (musicindex ADR 0049 R-QUERY-5).
  const withSpaces = (text) => text.replace(/["\\]/g, ' ');

  // A word that has two parts after withSpaces gives two words. Null means that
  // no part remains.
  const indexWord = (word) => {
    const parts = [];
    let part = '';
    for (const ch of withSpaces(word)) {
      if (rules.isSpace(ch)) {
        if (part !== '') parts.push(part);
        part = '';
      } else {
        part += ch;
      }
    }
    if (part !== '') parts.push(part);
    return parts.length === 0 ? null : parts.join(' ');
  };

  // The text of withSpaces in double quotes. Null means that the phrase has no text.
  const indexPhrase = (phrase) => {
    const spaced = withSpaces(phrase);
    return trim(spaced) === '' ? null : `"${spaced}"`;
  };

  const indexTerm = (term) => (term.kind === 'word' ? indexWord(term.text) : indexPhrase(term.text));

  // A term holds no offset, so the raw text tells if a field value has double quotes.
  const valueIsQuoted = (term) => term.raw[term.raw.indexOf(':') + 1] === '"';

  // A value in double quotes with a white space, a quote or a backslash is a
  // phrase. Each other value follows the rule of a word.
  const indexText = (term) => {
    const { text } = term.value;
    return valueIsQuoted(term) && [...text].some((ch) => rules.isSpace(ch) || ch === '"' || ch === '\\')
      ? indexPhrase(text)
      : indexWord(text);
  };

  const indexQ = (terms) => {
    const parts = terms
      .filter((term) => !term.excluded && term.kind !== 'field')
      .map(indexTerm)
      .filter((part) => part !== null);
    return parts.length === 0 ? null : parts.join(' ');
  };

  // The kind of a release (musicindex ADR 0020 R-WORD-1). `stated` is the value that
  // the feed states. It wins when it names a kind. Without a stated value, the count
  // rule decides. A count that no kind holds, such as zero, gives `album`.
  const releaseKind = (stated, trackCount) => {
    if (stated !== null && stated !== undefined) {
      const word = rules.lowerAscii(trim(stated));
      if (Object.hasOwn(TRACK_COUNTS, word)) return word;
    }
    return ['single', 'ep', 'album'].find((kind) => inCounts(TRACK_COUNTS[kind], trackCount)) ?? 'album';
  };

  const runnable = (terms) => terms
    .filter((term) => !term.excluded)
    .some((term) => (term.kind === 'field'
      ? term.state === 'valid' && term.field !== 'in:'
      : rules.isAlphanumeric(term.text)));

  const emptyFilters = () => ({
    artist: null,
    title: null,
    release: null,
    kinds: [],
    after: null,
    before: null,
    min_duration: null,
    max_duration: null,
    not_artist: [],
    not_title: [],
    not_release: [],
    not_q: [],
  });

  // Returns null when the kind: terms share no kind, or when their kinds send no
  // Index request.
  const filtersOf = (terms) => {
    const filters = emptyFilters();
    for (const term of terms) {
      if (term.kind !== 'field') {
        const text = term.excluded ? indexTerm(term) : null;
        if (text !== null) filters.not_q.push(text);
        continue;
      }
      if (term.state !== 'valid') continue;
      const name = fieldName(term);
      if (name === 'artist' || name === 'title' || name === 'release') {
        const text = indexText(term);
        if (text === null) continue;
        if (term.excluded) filters[`not_${name}`].push(text);
        else filters[name] = filters[name] === null ? text : `${filters[name]} ${text}`;
      } else if (term.excluded) {
        continue;
      } else if (name === 'kind') {
        filters.kinds = filters.kinds.length === 0
          ? [...term.value.kinds]
          : filters.kinds.filter((kind) => term.value.kinds.includes(kind));
        if (filters.kinds.length === 0) return null;
      } else if (name === 'after') {
        if (filters.after === null || term.value.first_day > filters.after) filters.after = term.value.first_day;
      } else if (name === 'before') {
        if (filters.before === null || term.value.last_day < filters.before) filters.before = term.value.last_day;
      } else if (name === 'duration') {
        const { min_secs: min, max_secs: max } = term.value;
        if (min !== null) filters.min_duration = filters.min_duration === null ? min : Math.max(filters.min_duration, min);
        if (max !== null) filters.max_duration = filters.max_duration === null ? max : Math.min(filters.max_duration, max);
      }
    }
    if (filters.kinds.length > 0 && kindPairs(filters.kinds) === null) return null;
    return filters;
  };

  const hasDuration = (filters) => filters.min_duration !== null || filters.max_duration !== null;

  const hasPositive = (filters) => filters.artist !== null
    || filters.title !== null
    || filters.release !== null
    || filters.kinds.length > 0
    || filters.after !== null
    || filters.before !== null
    || hasDuration(filters);

  const hasExclusion = (filters) => filters.not_artist.length > 0
    || filters.not_title.length > 0
    || filters.not_release.length > 0
    || filters.not_q.length > 0;

  const pairsOf = (filters) => {
    const pairs = [];
    for (const name of ['artist', 'title', 'release']) {
      if (filters[name] !== null) pairs.push([name, filters[name]]);
    }
    for (const name of ['not_artist', 'not_title', 'not_release', 'not_q']) {
      for (const value of filters[name]) pairs.push([name, value]);
    }
    pairs.push(...(kindPairs(filters.kinds) ?? []));
    if (filters.after !== null) pairs.push(['after', filters.after]);
    if (filters.before !== null) pairs.push(['before', filters.before]);
    if (filters.min_duration !== null) pairs.push(['min_duration', String(filters.min_duration)]);
    if (filters.max_duration !== null) pairs.push(['max_duration', String(filters.max_duration)]);
    return pairs;
  };

  const requestOf = (q, filters) => {
    if (q === null && !hasPositive(filters)) return null;
    return { q, params: pairsOf(filters) };
  };

  const plan = (terms) => {
    const queryScope = scope(terms);
    if (queryScope === 'conflict' || queryScope === 'library_only') return { skip: queryScope };
    const tokens = [];
    for (const term of validFields(terms)) {
      if (term.excluded && ['kind:', 'after:', 'before:', 'duration:'].includes(term.field) && !tokens.includes(term.field)) {
        tokens.push(term.field);
      }
    }
    if (tokens.length > 0) return { skip: 'unsupported', tokens };
    const filters = filtersOf(terms);
    if (filters === null) return { skip: 'no_index_part' };
    const q = indexQ(terms);
    if (q === null && !hasPositive(filters)) {
      return { skip: hasExclusion(filters) ? 'exclusions_only' : 'no_index_part' };
    }
    const blocksPublisher = filters.release !== null
      || filters.kinds.length > 0
      || filters.after !== null
      || filters.before !== null
      || hasDuration(filters);
    return {
      feed: hasDuration(filters) ? null : requestOf(q, filters),
      track: requestOf(q, filters),
      publisher: blocksPublisher ? null : requestOf(q, { ...filters, not_release: [] }),
    };
  };

  // The raw terms of the text, with the UTF-16 offsets that the cursor needs. This
  // is the lexer of the crate, so each offset agrees with its byte offset.
  const rawTerms = (text) => {
    const terms = [];
    let pos = 0;
    while (pos < text.length) {
      if (rules.isSpace(text[pos])) {
        pos += 1;
        continue;
      }
      const excluded = text[pos] === '-';
      const bodyStart = excluded ? pos + 1 : pos;
      const term = { start: pos, bodyStart, excluded };
      if (bodyStart >= text.length || rules.isSpace(text[bodyStart])) {
        terms.push({ ...term, end: bodyStart, shape: 'empty' });
        pos = bodyStart;
        continue;
      }
      if (text[bodyStart] === '"') {
        const quoted = readQuoted(text, bodyStart);
        terms.push({ ...term, end: quoted.end, shape: 'phrase' });
        pos = quoted.end;
        continue;
      }
      let wordEnd = bodyStart;
      while (wordEnd < text.length && !rules.isSpace(text[wordEnd])) wordEnd += 1;
      const word = text.slice(bodyStart, wordEnd);
      const colon = word.indexOf(':');
      const name = colon >= 0 ? fieldOf(word.slice(0, colon)) : null;
      if (name === null) {
        terms.push({ ...term, end: wordEnd, shape: 'word', word });
        pos = wordEnd;
        continue;
      }
      const colonAt = bodyStart + colon;
      const valueStart = colonAt + 1;
      const quoted = text[valueStart] === '"';
      const read = quoted ? readQuoted(text, valueStart) : null;
      const end = quoted ? read.end : wordEnd;
      const value = quoted ? read.content : text.slice(valueStart, wordEnd);
      terms.push({ ...term, end, shape: 'field', name, colon: colonAt, valueStart, quoted, value });
      pos = end;
    }
    return terms;
  };

  // The text of a value prefix that a person types. A quote at the start opens the
  // value, and the escapes after it read as in the parser.
  const unquoteValuePrefix = (prefix) => {
    if (!prefix.startsWith('"')) return prefix;
    let out = '';
    let at = 1;
    while (at < prefix.length) {
      const ch = prefix[at];
      if (ch === '"') break;
      if (ch === '\\') {
        const next = prefix[at + 1];
        if (next === '"' || next === '\\') out += next;
        else if (next === undefined) out += '\\';
        else out += `\\${next}`;
        at += 2;
        continue;
      }
      out += ch;
      at += 1;
    }
    return out;
  };

  // The term under the cursor. `cursor` counts UTF-16 code units. A cursor in a
  // surrogate pair moves back to the start of the pair.
  const cursorTerm = (text, at) => {
    let cursor = Math.max(0, Math.min(at, text.length));
    if (cursor > 0 && cursor < text.length && /[\uD800-\uDBFF]/.test(text[cursor - 1]) && /[\uDC00-\uDFFF]/.test(text[cursor])) {
      cursor -= 1;
    }
    const raw = rawTerms(text).find((term) => term.start <= cursor && cursor <= term.end);
    if (raw === undefined) return { cursor, raw: null, range: [cursor, cursor], partial: { kind: 'name', prefix: '' } };
    const typedEnd = Math.max(cursor, raw.bodyStart);
    const named = (end) => ({ kind: 'name', prefix: text.slice(raw.bodyStart, end) });
    let partial;
    if (raw.shape === 'empty') partial = { kind: 'name', prefix: '' };
    else if (raw.shape === 'phrase') partial = { kind: 'other' };
    else if (raw.shape === 'word') partial = raw.word.includes(':') ? { kind: 'other' } : named(typedEnd);
    else if (typedEnd <= raw.colon) partial = named(typedEnd);
    else {
      const typed = Math.min(Math.max(typedEnd, raw.valueStart), raw.end);
      let start = raw.valueStart;
      let end = raw.end;
      // A quoted value is one item. Its commas do not split it.
      if (raw.name === 'kind' && !raw.quoted) {
        start = raw.valueStart + text.slice(raw.valueStart, typed).lastIndexOf(',') + 1;
        const comma = text.slice(typed, raw.end).indexOf(',');
        end = comma < 0 ? raw.end : typed + comma;
      }
      partial = { kind: 'value', name: raw.name, prefix: text.slice(start, typed), range: [start, end] };
    }
    return { cursor, raw, range: [raw.bodyStart, raw.end], partial };
  };

  // The rows of the field list for `text` and the UTF-16 offset `cursor` (musicindex
  // ADR 0052 R-QUERY-7 item 2). `library` is `present` or `absent`. The result has
  // `edit`, the range that a row replaces, or null. It has `rows`, each a field
  // token or a field token with a value word. It has `names`, the field and the
  // typed prefix of the name rows that the app gives, or null. It has `error`, the
  // message of the field under the cursor, or null.
  const fieldListRows = (text, cursor, library) => {
    showsField(library, 'artist');
    const term = cursorTerm(text, cursor);
    let error = null;
    for (const raw of rawTerms(text)) {
      if (raw.shape !== 'field' || raw.start > term.cursor || term.cursor > raw.end) continue;
      if (!showsField(library, raw.name)) continue;
      const state = valueState(raw.name, raw.value, raw.excluded);
      if (state.state === 'invalid') {
        error = state.error.message;
        break;
      }
    }
    const { partial } = term;
    if (partial.kind === 'name') {
      const start = term.range[0];
      const prefix = partial.prefix.toLowerCase();
      return {
        edit: { start, end: Math.max(term.cursor, start) },
        rows: fieldTable
          .filter((row) => row.token.startsWith(prefix) && showsField(library, row.token.slice(0, -1)))
          .map((row) => ({ field: row.token })),
        names: null,
        error,
      };
    }
    if (partial.kind === 'value') {
      const shown = showsField(library, partial.name);
      const typed = unquoteValuePrefix(partial.prefix);
      const lower = typed.toLowerCase();
      const token = `${partial.name}:`;
      return {
        edit: { start: partial.range[0], end: partial.range[1] },
        rows: shown
          ? wordsOf(partial.name).filter((word) => word.startsWith(lower)).map((value) => ({ field: token, value }))
          : [],
        names: shown && NAME_FIELDS.includes(partial.name) ? { field: token, prefix: typed } : null,
        error,
      };
    }
    return { edit: null, rows: [], names: null, error };
  };

  // The note of the terms and their plan, or null when the Index part has no note
  // (musicindex ADR 0052 R-QUERY-8). `library` is `present` or `absent`. The first
  // field error comes first. An app with a Library gets no note for a query of
  // the Library only, because its Library rows show.
  const searchNote = (terms, queryPlan, library) => {
    showsField(library, 'artist');
    const failed = terms.find((term) => term.kind === 'field' && term.state === 'invalid');
    if (failed) return note('This search cannot run', failed.error.message);
    switch (queryPlan.skip) {
      case undefined:
        return runnable(terms) ? null : NEEDS_A_WORD;
      case 'library_only':
        return library === 'present'
          ? null
          : note('This search uses a Library field', 'Remove is:, playlist:, has: or in:library to search Music Index.');
      case 'conflict':
        return note('No results', 'The search asks for the Library and for Music Index only. Remove one of the fields.');
      case 'unsupported':
        return note(
          queryPlan.tokens.length === 0
            ? 'Music Index cannot apply this search yet'
            : `Music Index cannot exclude ${tokenList(queryPlan.tokens)} yet`,
          'Remove the - from the field.',
        );
      case 'no_index_part':
        // A term that can run, with no request: the `kind:` values send none.
        return runnable(terms) ? note('No results', 'Music Index has no rows for these fields.') : NEEDS_A_WORD;
      default:
        return NEEDS_A_WORD;
    }
  };

  return { parse, scope, indexQ, runnable, plan, releaseKind, fieldListRows, searchNote };
}
