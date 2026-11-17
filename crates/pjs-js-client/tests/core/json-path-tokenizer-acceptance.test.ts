/**
 * Acceptance tests for #685: one scanner core behind `tokenizeJsonPath` and the
 * allocation-free `isValidJsonPath`. Tokenizer outputs and error texts are the
 * pre-change ones; `isValidJsonPath(p)` equals `tokenizeJsonPath(p).ok`.
 */

import { describe, test, expect } from '@jest/globals';
import {
  isValidJsonPath,
  tokenizeJsonPath,
  type JsonPathSegment
} from '../../src/core/json-path.js';

const key = (value: string): JsonPathSegment => ({ kind: 'key', key: value });
const index = (value: number): JsonPathSegment => ({ kind: 'index', index: value });

describe('tokenizeJsonPath', () => {
  const zeros = '0'.repeat(400);

  test.each<[string, JsonPathSegment[]]>([
    ['$', []],
    ['$.a', [key('a')]],
    ['$[0]', [index(0)]],
    ['$.a[0].b', [key('a'), index(0), key('b')]],
    ['$[01]', [index(1)]],
    ['$[9007199254740991]', [index(9007199254740991)]],
    [`$[${zeros}1]`, [index(1)]],
    ['$.ключ', [key('ключ')]],
    ['$.a b', [key('a b')]],
    ['$.\uD800', [key('\uD800')]],
    ['$[0][1]', [index(0), index(1)]]
  ])('tokenizes %s', (path, segments) => {
    expect(tokenizeJsonPath(path)).toEqual({ ok: true, segments });
  });

  test.each<[string, string]>([
    ['', 'Path must start with "$."'],
    ['x', 'Path must start with "$."'],
    ['$a', 'Unexpected character "a" at position 1'],
    ['$[0]x', 'Unexpected character "x" at position 4'],
    ['$.a]', 'Unexpected character "]" at position 3'],
    ['$.', 'Empty segment at position 1'],
    ['$..a', 'Empty segment at position 1'],
    ['$.a.', 'Empty segment at position 3'],
    ['$[abc', 'Unterminated index at position 1'],
    ['$.a[0', 'Unterminated index at position 3'],
    ['$[1a]', 'Invalid array index "1a" at position 1'],
    ['$[1.5]', 'Invalid array index "1.5" at position 1'],
    ['$[-1]', 'Invalid array index "-1" at position 1'],
    ['$[]', 'Invalid array index "" at position 1'],
    ['$[9007199254740992]', 'Invalid array index "9007199254740992" at position 1']
  ])('rejects %j with its exact error', (path, error) => {
    expect(tokenizeJsonPath(path)).toEqual({ ok: false, error });
  });
});

describe('isValidJsonPath', () => {
  const alphabet = ['$', '.', '[', ']', '0', '1', 'a', '-'];

  function* corpus(): Generator<string> {
    let level: string[] = [''];
    for (let length = 0; length <= 4; length++) {
      for (const text of level) yield `$${text}`;
      level = level.flatMap((text) => alphabet.map((char) => text + char));
    }
  }

  test.each([
    '$',
    '$.a',
    '$[0]',
    '$.a[0].b',
    '$[01]',
    '$[9007199254740991]',
    `$[${'0'.repeat(400)}1]`,
    '$.\uD800',
    '$.ключ'
  ])('accepts %j', (path) => {
    expect(isValidJsonPath(path)).toBe(true);
  });

  test.each([
    '',
    'x',
    '$a',
    '$[0]x',
    '$.a]',
    '$.',
    '$..a',
    '$.a.',
    '$[abc',
    '$.a[0',
    '$[1a]',
    '$[1.5]',
    '$[-1]',
    '$[]',
    '$[9007199254740992]'
  ])('rejects %j', (path) => {
    expect(isValidJsonPath(path)).toBe(false);
  });

  test('equals tokenizeJsonPath(...).ok for every short path over the grammar alphabet', () => {
    let checked = 0;
    for (const path of corpus()) {
      expect({ path, valid: isValidJsonPath(path) }).toEqual({
        path,
        valid: tokenizeJsonPath(path).ok
      });
      checked++;
    }

    expect(checked).toBeGreaterThan(4000);
  });
});
