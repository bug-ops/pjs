/**
 * Acceptance tests for #666 (multi-index and root-index paths, strict index
 * resolution) and #668 (one key grammar, root `$` in `parseJsonPath`).
 */

import { describe, test, expect } from '@jest/globals';
import { JsonReconstructor } from '../../src/core/json-reconstructor.js';
import { FrameProcessor } from '../../src/core/frame-processor.js';
import { parseJsonPath } from '../../src/utils/index.js';
import { FrameType, Priority, PatchFrame, SkeletonFrame } from '../../src/types/index.js';

type Op = 'set' | 'append' | 'merge' | 'delete';
type PatchSpec = { operation: Op; path: string; value?: unknown };

function skeleton(payload: unknown): SkeletonFrame {
  return {
    frame_type: FrameType.Skeleton,
    priority: Priority.Critical,
    payload,
    complete: false,
    timestamp: 0
  } as SkeletonFrame;
}

function patchFrame(patches: PatchSpec[]): PatchFrame {
  return {
    frame_type: FrameType.Patch,
    priority: Priority.High,
    payload: { patches },
    timestamp: 0
  } as unknown as PatchFrame;
}

function apply(initial: unknown, patches: PatchSpec[]) {
  const reconstructor = new JsonReconstructor();
  reconstructor.processSkeleton(skeleton(initial));
  const result = reconstructor.applyPatch(patchFrame(patches));
  return { result, state: reconstructor.getCurrentState() };
}

describe('FrameProcessor path validation', () => {
  const processor = new FrameProcessor();
  const validates = (path: string): boolean =>
    processor.validateFrame(patchFrame([{ operation: 'set', path, value: 1 }])).isValid;

  test.each([
    '$',
    '$[0]',
    '$[0].a',
    '$.m[0]',
    '$.m[0][1]',
    '$.m[0][0].x',
    '$.user-id',
    '$.a b',
    '$[0][1][2]'
  ])('accepts %s', (path) => {
    expect(validates(path)).toBe(true);
  });

  test.each([
    '$.',
    '$..a',
    '$.a[]',
    '$.a[-1]',
    '$.a[1x]',
    '$.a[9007199254740992]',
    '$a',
    'a.b',
    ''
  ])('rejects %s', (path) => {
    expect(validates(path)).toBe(false);
  });
});

describe('JsonReconstructor index paths', () => {
  test('sets into a nested array element through several indices', () => {
    const { result, state } = apply({ m: [[{ x: 0 }]] }, [
      { operation: 'set', path: '$.m[0][0].x', value: 1 }
    ]);

    expect(result.skippedPatches).toBe(0);
    expect(state).toEqual({ m: [[{ x: 1 }]] });
    expect(Object.keys(state)).toEqual(['m']);
  });

  test('sets an element addressed by consecutive indices', () => {
    const { result, state } = apply({ m: [[1, 2], [3, 4]] }, [
      { operation: 'set', path: '$.m[1][0]', value: 9 }
    ]);

    expect(result.skippedPatches).toBe(0);
    expect(state).toEqual({ m: [[1, 2], [9, 4]] });
  });

  test('sets through a root index on an array root', () => {
    const { result, state } = apply([{ a: 0 }], [
      { operation: 'set', path: '$[0].a', value: 1 }
    ]);

    expect(result.skippedPatches).toBe(0);
    expect(state).toEqual([{ a: 1 }]);
  });

  test('sets an array root element directly', () => {
    const { result, state } = apply([1, 2], [{ operation: 'set', path: '$[1]', value: 5 }]);

    expect(result.skippedPatches).toBe(0);
    expect(state).toEqual([1, 5]);
  });

  test('root set followed by an indexed patch rebuilds an array root', () => {
    const { result, state } = apply([], [
      { operation: 'set', path: '$', value: [{ a: 0 }, { a: 2 }] },
      { operation: 'set', path: '$[0].a', value: 1 }
    ]);

    expect(result.skippedPatches).toBe(0);
    expect(state).toEqual([{ a: 1 }, { a: 2 }]);
  });
});

describe('JsonReconstructor strict index resolution', () => {
  test('skips a final index past the end of the array', () => {
    const { result, state } = apply({ a: [1] }, [{ operation: 'set', path: '$.a[5]', value: 9 }]);

    expect(result.skippedPatches).toBe(1);
    expect(result.appliedPatches).toBe(0);
    expect(state).toEqual({ a: [1] });
  });

  test('skips an intermediate index past the end of the array', () => {
    const { result, state } = apply({ a: [{ x: 0 }] }, [
      { operation: 'set', path: '$.a[3].x', value: 1 }
    ]);

    expect(result.skippedPatches).toBe(1);
    expect(state).toEqual({ a: [{ x: 0 }] });
  });

  test('skips a huge index without creating a sparse array', () => {
    const { result, state } = apply({ a: [] }, [
      { operation: 'set', path: '$.a[4294967294]', value: 1 }
    ]);

    expect(result.skippedPatches).toBe(1);
    expect(state).toEqual({ a: [] });
    expect((state as { a: unknown[] }).a).toHaveLength(0);
  });

  test('does not auto-create an array for an index under a missing key', () => {
    const { result, state } = apply({}, [{ operation: 'set', path: '$.a[0]', value: 1 }]);

    expect(result.skippedPatches).toBe(1);
    expect(state).toEqual({});
  });

  test('skips an index applied to a non-array value', () => {
    const { result, state } = apply({ a: { b: 1 } }, [
      { operation: 'set', path: '$.a[0]', value: 1 }
    ]);

    expect(result.skippedPatches).toBe(1);
    expect(state).toEqual({ a: { b: 1 } });
  });

  test('skips a root index on an object root', () => {
    const { result, state } = apply({ a: 1 }, [{ operation: 'set', path: '$[0]', value: 1 }]);

    expect(result.skippedPatches).toBe(1);
    expect(state).toEqual({ a: 1 });
  });

  test('does not write a literal key for an unparsable index segment', () => {
    const { result, state } = apply({ a: [1] }, [
      { operation: 'set', path: '$.a[1x]', value: 9 },
      { operation: 'set', path: '$.a[-1]', value: 9 },
      { operation: 'set', path: '$.a[9007199254740992]', value: 9 }
    ]);

    expect(result.skippedPatches).toBe(3);
    expect(state).toEqual({ a: [1] });
  });

  test('counts applied and skipped patches of one frame separately', () => {
    const { result, state } = apply({ a: [0, 0] }, [
      { operation: 'set', path: '$.a[0]', value: 1 },
      { operation: 'set', path: '$.a[2]', value: 1 },
      { operation: 'set', path: '$.a[1]', value: 1 }
    ]);

    expect(result.appliedPatches).toBe(2);
    expect(result.skippedPatches).toBe(1);
    expect(state).toEqual({ a: [1, 1] });
  });
});

describe('parseJsonPath', () => {
  test.each([
    ['$', []],
    ['$.user.name', ['user', 'name']],
    ['$.users[0].profile', ['users[0]', 'profile']],
    ['$.user-id', ['user-id']],
    ['$.a b', ['a b']],
    ['$[0].a', ['[0]', 'a']],
    ['$.m[0][1]', ['m[0][1]']],
    ['$.user.profile[0].name', ['user', 'profile[0]', 'name']]
  ])('accepts %s', (path, segments) => {
    const parsed = parseJsonPath(path);

    expect(parsed.isValid).toBe(true);
    expect(parsed.errors).toEqual([]);
    expect(parsed.segments).toEqual(segments);
  });

  test.each(['$.', '$..a', '$.a[]', '$.a[-1]', '$.a[9007199254740992]', 'user.name', '$a', ''])(
    'rejects %s with one message',
    (path) => {
      const parsed = parseJsonPath(path);

      expect(parsed.isValid).toBe(false);
      expect(parsed.errors).toHaveLength(1);
    }
  );

  test('accepts the largest safe-integer index', () => {
    expect(parseJsonPath('$.a[9007199254740991]').isValid).toBe(true);
  });

  test('agrees with FrameProcessor on every sample path', () => {
    const processor = new FrameProcessor();
    const paths = ['$', '$[0]', '$.m[0][1]', '$.user-id', '$.a b', '$.', '$..a', '$.a[-1]', 'x'];

    for (const path of paths) {
      const frameValid = processor.validateFrame(
        patchFrame([{ operation: 'set', path, value: 1 }])
      ).isValid;
      expect(parseJsonPath(path).isValid).toBe(frameValid);
    }
  });
});
