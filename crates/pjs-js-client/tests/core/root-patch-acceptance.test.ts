/**
 * Acceptance tests for #657: root `$` patches.
 *
 * Root `set`/`merge` frames remain valid input (non-object roots, whole-value
 * replacement); keys with `.`, `[`, `]` or empty are now addressed by quoted
 * path segments instead (see reconstructor-own-keys-acceptance.test.ts).
 */

import { describe, test, expect, beforeEach } from '@jest/globals';
import { JsonReconstructor } from '../../src/core/json-reconstructor.js';
import { FrameProcessor } from '../../src/core/frame-processor.js';
import {
  FrameType,
  Priority,
  PatchFrame,
  SkeletonFrame
} from '../../src/types/index.js';

type Op = 'set' | 'append' | 'merge' | 'delete';

function skeleton(payload: unknown): SkeletonFrame {
  return {
    frame_type: FrameType.Skeleton,
    priority: Priority.Critical,
    payload,
    complete: false,
    timestamp: 0
  } as SkeletonFrame;
}

function patchFrame(patches: Array<{ operation: Op; path: string; value?: unknown }>): PatchFrame {
  return {
    frame_type: FrameType.Patch,
    priority: Priority.High,
    payload: { patches },
    timestamp: 0
  } as unknown as PatchFrame;
}

describe('JsonReconstructor.applyPatch root path', () => {
  let reconstructor: JsonReconstructor;

  beforeEach(() => {
    reconstructor = new JsonReconstructor();
  });

  test('applies a root set patch and a following field patch (issue repro)', () => {
    reconstructor.processSkeleton(skeleton({ 'a.b': {}, ok: 0 }));

    const result = reconstructor.applyPatch(
      patchFrame([
        { operation: 'set', path: '$', value: { 'a.b': { c: 1 } } },
        { operation: 'set', path: '$.ok', value: 2 }
      ])
    );

    expect(result.skippedPatches).toBe(0);
    expect(result.appliedPatches).toBe(2);
    expect(reconstructor.getCurrentState()).toEqual({ 'a.b': { c: 1 }, ok: 2 });
  });

  test('root set replaces the whole state rather than merging into it', () => {
    reconstructor.processSkeleton(skeleton({ stale: 1, 'a.b': 0 }));

    const result = reconstructor.applyPatch(
      patchFrame([{ operation: 'set', path: '$', value: { 'a.b': 'v' } }])
    );

    expect(result.skippedPatches).toBe(0);
    expect(reconstructor.getCurrentState()).toEqual({ 'a.b': 'v' });
  });

  test('root set keeps reconstruction equal to the input for nested unencodable keys', () => {
    const input = { x: { 'a[0]': 'v' }, '': 1, 'k]': [1, 2], ok: 3 };
    reconstructor.processSkeleton(skeleton({ x: {}, '': 0, 'k]': [], ok: 0 }));

    const result = reconstructor.applyPatch(
      patchFrame([
        { operation: 'set', path: '$', value: { x: { 'a[0]': 'v' }, '': 1, 'k]': [1, 2] } },
        { operation: 'set', path: '$.ok', value: 3 }
      ])
    );

    expect(result.skippedPatches).toBe(0);
    expect(reconstructor.getCurrentState()).toEqual(input);
  });

  test('root set deep-clones the value so later mutation of the frame does not leak', () => {
    reconstructor.processSkeleton(skeleton({}));
    const value = { 'a.b': { c: 1 } };

    reconstructor.applyPatch(patchFrame([{ operation: 'set', path: '$', value }]));
    value['a.b'].c = 99;

    expect(reconstructor.getCurrentState()).toEqual({ 'a.b': { c: 1 } });
  });

  test('root set keeps the state prototype-free for a __proto__ key', () => {
    reconstructor.processSkeleton(skeleton({}));
    const value = JSON.parse('{"__proto__": {"polluted": true}}');

    const result = reconstructor.applyPatch(
      patchFrame([{ operation: 'set', path: '$', value }])
    );

    expect(result.skippedPatches).toBe(0);
    expect(({} as Record<string, unknown>).polluted).toBeUndefined();
    const state = reconstructor.getCurrentState();
    expect(Object.getPrototypeOf(state)).toBeNull();
    expect(Object.keys(state)).toEqual(['__proto__']);
  });

  test('root merge on objects shallow-merges the value into the state', () => {
    reconstructor.processSkeleton(skeleton({ keep: 1, over: 0, nested: { a: 1 } }));

    const result = reconstructor.applyPatch(
      patchFrame([{ operation: 'merge', path: '$', value: { over: 2, 'a.b': 3, nested: { b: 2 } } }])
    );

    expect(result.skippedPatches).toBe(0);
    expect(result.appliedPatches).toBe(1);
    expect(reconstructor.getCurrentState()).toEqual({
      keep: 1,
      over: 2,
      'a.b': 3,
      nested: { b: 2 }
    });
  });

  test('root merge with a non-object value replaces the state', () => {
    reconstructor.processSkeleton(skeleton({ a: 1 }));

    const result = reconstructor.applyPatch(
      patchFrame([{ operation: 'merge', path: '$', value: [1, 2] }])
    );

    expect(result.skippedPatches).toBe(0);
    expect(reconstructor.getCurrentState()).toEqual([1, 2]);
  });

  test.each<Op>(['append', 'delete'])('root %s stays skipped and leaves the state unchanged', (operation) => {
    reconstructor.processSkeleton(skeleton({ a: 1 }));

    const result = reconstructor.applyPatch(
      patchFrame([{ operation, path: '$', value: 5 }])
    );

    expect(result.skippedPatches).toBe(1);
    expect(result.appliedPatches).toBe(0);
    expect(reconstructor.getCurrentState()).toEqual({ a: 1 });
  });

  test('paths that are neither $ nor start with $. are still skipped', () => {
    reconstructor.processSkeleton(skeleton({ a: 1 }));

    const result = reconstructor.applyPatch(
      patchFrame([
        { operation: 'set', path: '$x', value: 1 },
        { operation: 'set', path: 'a', value: 1 }
      ])
    );

    expect(result.skippedPatches).toBe(2);
    expect(reconstructor.getCurrentState()).toEqual({ a: 1 });
  });
});

describe('FrameProcessor.validateFrame path grammar', () => {
  let processor: FrameProcessor;

  beforeEach(() => {
    processor = new FrameProcessor();
  });

  test.each(['$', '$.ok', '$.user-id', '$.a b', '$.naïve', '$.items[0]', '$.a.user-id[3]'])(
    'accepts patch path %s',
    (path) => {
      const result = processor.validateFrame(patchFrame([{ operation: 'set', path, value: 1 }]));

      expect(result.errors).toEqual([]);
      expect(result.isValid).toBe(true);
    }
  );

  test.each(['', 'a', '$.', '$..a', '$.a.', '$.a[', '$.a]', '$.a[x]', '$.a[0', '$[0]x', '$a'])(
    'rejects patch path %j',
    (path) => {
      const result = processor.validateFrame(patchFrame([{ operation: 'set', path, value: 1 }]));

      expect(result.isValid).toBe(false);
    }
  );
});
