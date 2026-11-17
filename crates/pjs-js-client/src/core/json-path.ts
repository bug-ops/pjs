/**
 * Internal path grammar shared by frame validation, `parseJsonPath` and the
 * reconstructor: `$` followed by any number of `.key` or `[n]` segments.
 *
 * A key is non-empty and contains none of `.`, `[`, `]` (the Rust
 * `JsonPath::is_valid_key` rule); an index is a decimal safe integer.
 */

/** One tokenized path segment. */
export type JsonPathSegment =
  | { readonly kind: 'key'; readonly key: string }
  | { readonly kind: 'index'; readonly index: number };

export type JsonPathParse =
  | { readonly ok: true; readonly segments: readonly JsonPathSegment[] }
  | { readonly ok: false; readonly error: string };

const DOT = 46;
const OPEN_BRACKET = 91;
const CLOSE_BRACKET = 93;
const ZERO = 48;
const NINE = 57;

function indexError(path: string, pos: number, close: number): string {
  return `Invalid array index "${path.slice(pos + 1, close)}" at position ${pos}`;
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
    } else if (code === OPEN_BRACKET) {
      const close = path.indexOf(']', pos);
      if (close === -1) return `Unterminated index at position ${pos}`;
      if (close === pos + 1) return indexError(path, pos, close);
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
