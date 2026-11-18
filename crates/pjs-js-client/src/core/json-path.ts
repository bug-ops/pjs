/**
 * Internal path grammar shared by frame validation, `parseJsonPath` and the
 * reconstructor (mirrors Rust `JsonPath::from_str`): `$` followed by any number
 * of `.plain`, `[index]` or `["quoted"]` segments.
 *
 * The one difference from Rust: indices above `Number.MAX_SAFE_INTEGER` are
 * rejected here, while Rust accepts up to `usize::MAX`. Past 2^53-1 a JS number
 * can no longer represent the integer exactly, so the index would silently
 * address a different element.
 *
 * A plain key is non-empty and contains none of `.`, `[`, `]`. An index is `0`
 * or `[1-9][0-9]*` up to `Number.MAX_SAFE_INTEGER`. A quoted key is the
 * canonical `JSON.stringify` literal of a key that is not plain and has no lone
 * surrogates.
 */

/** One tokenized path segment. */
export type JsonPathSegment =
  | { readonly kind: 'key'; readonly key: string }
  | { readonly kind: 'index'; readonly index: number };

export type JsonPathParse =
  | { readonly ok: true; readonly segments: readonly JsonPathSegment[] }
  | { readonly ok: false; readonly error: string };

const DOT = 46;
const QUOTE = 34;
const BACKSLASH = 92;
const OPEN_BRACKET = 91;
const CLOSE_BRACKET = 93;
const ZERO = 48;
const NINE = 57;

function indexError(path: string, pos: number, close: number): string {
  return `Invalid array index "${path.slice(pos + 1, close)}" at position ${pos}`;
}

function isPlainKey(key: string): boolean {
  return key.length > 0 && !/[.[\]]/.test(key);
}

function hasLoneSurrogate(text: string): boolean {
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = text.charCodeAt(i + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) return true;
      i++;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      return true;
    }
  }
  return false;
}

/** Scans a `["..."]` segment whose `[` is at `pos`; returns the key and the position after `]`. */
function scanQuotedKey(
  path: string,
  pos: number
): { readonly key: string; readonly next: number } | string {
  const start = pos + 1;
  let end = start + 1;
  while (end < path.length && path.charCodeAt(end) !== QUOTE) {
    end += path.charCodeAt(end) === BACKSLASH ? 2 : 1;
  }
  if (end >= path.length) return `Unterminated quoted key at position ${pos}`;

  const literal = path.slice(start, end + 1);
  let key: unknown;
  try {
    key = JSON.parse(literal);
  } catch {
    return `Invalid quoted key at position ${pos}`;
  }
  if (
    typeof key !== 'string' ||
    JSON.stringify(key) !== literal ||
    isPlainKey(key) ||
    hasLoneSurrogate(key)
  ) {
    return `Invalid quoted key at position ${pos}`;
  }
  if (path.charCodeAt(end + 1) !== CLOSE_BRACKET) {
    return `Unterminated quoted key at position ${pos}`;
  }
  return { key, next: end + 2 };
}

/**
 * Single scanner behind `tokenizeJsonPath` and `isValidJsonPath`. Returns the
 * error text, or `null` for a valid path; appends segments to `out` when given.
 */
function scanJsonPath(path: string, out: JsonPathSegment[] | null): string | null {
  if (!path.startsWith('$')) {
    return 'Path must start with "$."';
  }

  const length = path.length;
  let pos = 1;

  while (pos < length) {
    const code = path.charCodeAt(pos);
    if (code === DOT) {
      let end = pos + 1;
      while (end < length) {
        const c = path.charCodeAt(end);
        if (c === DOT || c === OPEN_BRACKET || c === CLOSE_BRACKET) break;
        end++;
      }
      if (end === pos + 1) return `Empty segment at position ${pos}`;
      out?.push({ kind: 'key', key: path.slice(pos + 1, end) });
      pos = end;
    } else if (code === OPEN_BRACKET && path.charCodeAt(pos + 1) === QUOTE) {
      const quoted = scanQuotedKey(path, pos);
      if (typeof quoted === 'string') return quoted;
      out?.push({ kind: 'key', key: quoted.key });
      pos = quoted.next;
    } else if (code === OPEN_BRACKET) {
      const close = path.indexOf(']', pos);
      if (close === -1) return `Unterminated index at position ${pos}`;
      if (close === pos + 1) return indexError(path, pos, close);
      if (path.charCodeAt(pos + 1) === ZERO && close > pos + 2) return indexError(path, pos, close);
      let index = 0;
      for (let i = pos + 1; i < close; i++) {
        const digit = path.charCodeAt(i) - ZERO;
        if (digit < 0 || digit > NINE - ZERO) return indexError(path, pos, close);
        index = index * 10 + digit;
        if (index > Number.MAX_SAFE_INTEGER) return indexError(path, pos, close);
      }
      out?.push({ kind: 'index', index });
      pos = close + 1;
    } else {
      return `Unexpected character "${path[pos]}" at position ${pos}`;
    }
  }

  return null;
}

/** Tokenizes `path` into typed segments; `$` yields an empty list. */
export function tokenizeJsonPath(path: string): JsonPathParse {
  const segments: JsonPathSegment[] = [];
  const error = scanJsonPath(path, segments);
  return error === null ? { ok: true, segments } : { ok: false, error };
}

/** Returns `true` when `path` satisfies the shared path grammar, without allocating segments. */
export function isValidJsonPath(path: string): boolean {
  return scanJsonPath(path, null) === null;
}

/** Formats typed segments back into a path string; inverse of {@link tokenizeJsonPath}. */
export function formatJsonPath(_segments: readonly JsonPathSegment[]): string {
  throw new Error('not implemented');
}
