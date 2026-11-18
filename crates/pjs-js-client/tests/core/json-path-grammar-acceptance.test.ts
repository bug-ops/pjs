/**
 * Acceptance tests for #713 (strict index grammar) and #698 (quoted keys).
 *
 * Grammar (mirrors Rust `JsonPath::from_str`): `$` then any of `.plain`,
 * `[index]`, `["quoted"]`. `index` is `0` or `[1-9][0-9]*` up to
 * `Number.MAX_SAFE_INTEGER`; `quoted` is the canonical `JSON.stringify` literal
 * of a key that is not plain (empty, or containing `.`, `[`, `]`), without lone
 * surrogates.
 */

import { describe, test, expect } from '@jest/globals';
import {
  isValidJsonPath,
  tokenizeJsonPath,
  type JsonPathSegment
} from '../../src/core/json-path.js';

const key = (value: string): JsonPathSegment => ({ kind: 'key', key: value });
const index = (value: number): JsonPathSegment => ({ kind: 'index', index: value });

describe('tokenizeJsonPath strict indices', () => {
  test('rejects signed, zero-padded and over-padded indices and keeps canonical ones', () => {
    const rejected = ['$[01]', '$[007]', '$[00]', `$[${'0'.repeat(400)}1]`, '$[+5]', '$[-0]'];
    const accepted: Array<[string, JsonPathSegment[]]> = [
      ['$[0]', [index(0)]],
      ['$[10]', [index(10)]],
      ['$[9007199254740991]', [index(9007199254740991)]],
      ['$.a[0][10].b', [key('a'), index(0), index(10), key('b')]]
    ];

    for (const [path, segments] of accepted) {
      expect(tokenizeJsonPath(path)).toEqual({ ok: true, segments });
    }
    for (const path of rejected) {
      expect([path, tokenizeJsonPath(path).ok, isValidJsonPath(path)]).toEqual([path, false, false]);
    }
  });
});

describe('tokenizeJsonPath quoted keys', () => {
  test.each<[string, JsonPathSegment[]]>([
    ['$["a.b"]', [key('a.b')]],
    ['$[""]', [key('')]],
    ['$["x]"]', [key('x]')]],
    ['$["[0]"]', [key('[0]')]],
    ['$["a\\"b.c"]', [key('a"b.c')]],
    ['$["a\\\\b.c"]', [key('a\\b.c')]],
    ['$["a\\nb.c"]', [key('a\nb.c')]],
    ['$["\\u0000.a"]', [key('\u0000.a')]],
    ['$["ключ.x"]', [key('ключ.x')]],
    ['$.x["a.b"][2].y', [key('x'), key('a.b'), index(2), key('y')]],
    ['$["a.b"]["c.d"]', [key('a.b'), key('c.d')]]
  ])('tokenizes %j', (path, segments) => {
    expect(tokenizeJsonPath(path)).toEqual({ ok: true, segments });
  });

  test('rejects non-canonical quoted forms and agrees with isValidJsonPath', () => {
    const rejected = [
      '$["a"]',
      '$["a.b"',
      '$["a.b" ]',
      '$[ "a.b"]',
      "$['a.b']",
      '$["\\u0041.b"]',
      '$["a\\/b.c"]',
      '$["\\ud800"]',
      '$["a.b"]x',
      '$["a.b""c"]',
      '$["a.b'
    ];

    expect(tokenizeJsonPath('$["a.b"]').ok).toBe(true);
    for (const path of rejected) {
      expect([path, tokenizeJsonPath(path).ok, isValidJsonPath(path)]).toEqual([path, false, false]);
    }
    for (const path of ['$["a.b"]', '$[""]', '$.a["b.c"][1]', '$[01]', '$[0]']) {
      expect(isValidJsonPath(path)).toBe(tokenizeJsonPath(path).ok);
    }
  });
});
