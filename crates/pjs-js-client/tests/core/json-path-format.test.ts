/**
 * Acceptance tests for #731: `formatJsonPath` is the internal inverse of
 * `tokenizeJsonPath`. `tokenizeJsonPath(formatJsonPath(s))` deep-equals `s`
 * for every key without a lone surrogate.
 */

import { describe, test, expect } from '@jest/globals';
import {
  formatJsonPath,
  tokenizeJsonPath,
  type JsonPathSegment
} from '../../src/core/json-path.js';

const key = (value: string): JsonPathSegment => ({ kind: 'key', key: value });
const index = (value: number): JsonPathSegment => ({ kind: 'index', index: value });

describe('formatJsonPath', () => {
  test.each<[string, JsonPathSegment[], string]>([
    ['the root', [], '$'],
    ['a plain key', [key('a')], '$.a'],
    ['a digit-only key stays a key', [key('0')], '$.0'],
    ['an index', [index(0)], '$[0]'],
    ['the largest safe index', [index(Number.MAX_SAFE_INTEGER)], '$[9007199254740991]'],
    ['key then index then key', [key('a'), index(3), key('b')], '$.a[3].b'],
    ['a key with a dot', [key('a.b')], '$["a.b"]'],
    ['an empty key', [key('')], '$[""]'],
    ['a key with brackets', [key('[x]')], '$["[x]"]'],
    ['a key with a quote and a dot', [key('a"b.c')], '$["a\\"b.c"]'],
    ['a non-ASCII plain key', [key('ключ')], '$.ключ'],
    ['a quoted key between plain ones', [key('a'), key('b.c'), key('d')], '$.a["b.c"].d']
  ])('formats %s', (_label, segments, expected) => {
    expect(formatJsonPath(segments)).toBe(expected);
  });
});

describe('formatJsonPath round trip', () => {
  const fragments = ['', 'a', '.', '[', ']', '"', '\\', ' ', '0', '$', 'é', '\u{1F600}', '\n', '\u0000'];

  function seededRandom(seed: number): () => number {
    let state = seed >>> 0;
    return () => {
      state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
      return state / 0x1_0000_0000;
    };
  }

  function randomKey(next: () => number): string {
    const parts = Math.floor(next() * 4);
    let text = '';
    for (let i = 0; i < parts; i++) {
      text += fragments[Math.floor(next() * fragments.length)];
    }
    return text;
  }

  function randomSegments(next: () => number): JsonPathSegment[] {
    const count = Math.floor(next() * 6);
    const segments: JsonPathSegment[] = [];
    for (let i = 0; i < count; i++) {
      segments.push(next() < 0.3 ? index(Math.floor(next() * 1000)) : key(randomKey(next)));
    }
    return segments;
  }

  test('tokenize(format(s)) equals s for 2000 seeded random segment lists', () => {
    const next = seededRandom(731);
    for (let i = 0; i < 2000; i++) {
      const segments = randomSegments(next);
      const path = formatJsonPath(segments);
      expect({ path, parsed: tokenizeJsonPath(path) }).toEqual({
        path,
        parsed: { ok: true, segments }
      });
    }
  });

  test('keys that look like path syntax survive the round trip', () => {
    const segments = [key('a.b'), key(''), key('[x]'), key('0'), key('$'), key('["q"]'), index(0)];

    expect(tokenizeJsonPath(formatJsonPath(segments))).toEqual({ ok: true, segments });
  });
});
