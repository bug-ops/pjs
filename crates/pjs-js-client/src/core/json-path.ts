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

const KEY_END = /[.[\]]/;
const DIGITS = /^\d+$/;

function fail(error: string): JsonPathParse {
  return { ok: false, error };
}

/** Tokenizes `path` into typed segments; `$` yields an empty list. */
export function tokenizeJsonPath(path: string): JsonPathParse {
  if (!path.startsWith('$')) {
    return fail('Path must start with "$."');
  }

  const segments: JsonPathSegment[] = [];
  let pos = 1;

  while (pos < path.length) {
    if (path[pos] === '.') {
      let end = pos + 1;
      while (end < path.length && !KEY_END.test(path[end])) end++;
      if (end === pos + 1) return fail(`Empty segment at position ${pos}`);
      segments.push({ kind: 'key', key: path.slice(pos + 1, end) });
      pos = end;
    } else if (path[pos] === '[') {
      const close = path.indexOf(']', pos);
      if (close === -1) return fail(`Unterminated index at position ${pos}`);
      const digits = path.slice(pos + 1, close);
      const index = DIGITS.test(digits) ? Number(digits) : NaN;
      if (!Number.isSafeInteger(index)) {
        return fail(`Invalid array index "${digits}" at position ${pos}`);
      }
      segments.push({ kind: 'index', index });
      pos = close + 1;
    } else {
      return fail(`Unexpected character "${path[pos]}" at position ${pos}`);
    }
  }

  return { ok: true, segments };
}

/** Returns `true` when `path` satisfies the shared path grammar. */
export function isValidJsonPath(_path: string): boolean {
  throw new Error('not implemented');
}
