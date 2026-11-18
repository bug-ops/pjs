/**
 * Acceptance tests for #731: root-level `append` / `append_str` and the new
 * `append_str` operation in `JsonReconstructor` (spec 073 FR-009 delta).
 */

import { describe, test, expect, beforeEach } from '@jest/globals';
import { JsonReconstructor } from '../../src/core/json-reconstructor.js';
import { FrameType, Priority, type PatchFrame, type SkeletonFrame } from '../../src/types/index.js';

type Op = PatchFrame['payload']['patches'][number]['operation'];

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

describe('JsonReconstructor root append', () => {
  let reconstructor: JsonReconstructor;

  beforeEach(() => {
    reconstructor = new JsonReconstructor();
  });

  test('pushes the array elements onto an array root', () => {
    reconstructor.processSkeleton(skeleton([]));

    const result = reconstructor.applyPatch(
      patchFrame([
        { operation: 'append', path: '$', value: [1, 2] },
        { operation: 'append', path: '$', value: [3] }
      ])
    );

    expect(result).toMatchObject({ appliedPatches: 2, skippedPatches: 0 });
    expect(reconstructor.getCurrentState()).toEqual([1, 2, 3]);
  });

  test('pushes a scalar once', () => {
    reconstructor.processSkeleton(skeleton([1]));

    reconstructor.applyPatch(patchFrame([{ operation: 'append', path: '$', value: 'x' }]));

    expect(reconstructor.getCurrentState()).toEqual([1, 'x']);
  });

  test('assembles an array root set to [] then appended', () => {
    reconstructor.processSkeleton(skeleton({}));

    reconstructor.applyPatch(
      patchFrame([
        { operation: 'set', path: '$', value: [] },
        { operation: 'append', path: '$', value: [{ a: 1 }, { b: 2 }] }
      ])
    );

    expect(reconstructor.getCurrentState()).toEqual([{ a: 1 }, { b: 2 }]);
  });

  test.each<[string, unknown]>([
    ['an object root', { a: 1 }],
    ['a string root', 'abc'],
    ['a number root', 7],
    ['a null root', null]
  ])('skips root append on %s', (_label, root) => {
    reconstructor.processSkeleton(skeleton(root));

    const result = reconstructor.applyPatch(patchFrame([{ operation: 'append', path: '$', value: [1] }]));

    expect(result).toMatchObject({ appliedPatches: 0, skippedPatches: 1 });
    expect(reconstructor.getCurrentState()).toEqual(root);
  });
});

describe('JsonReconstructor append_str', () => {
  let reconstructor: JsonReconstructor;

  beforeEach(() => {
    reconstructor = new JsonReconstructor();
  });

  test('concatenates onto a string root', () => {
    reconstructor.processSkeleton(skeleton({}));

    const result = reconstructor.applyPatch(
      patchFrame([
        { operation: 'set', path: '$', value: '' },
        { operation: 'append_str', path: '$', value: 'hello ' },
        { operation: 'append_str', path: '$', value: 'world' }
      ])
    );

    expect(result).toMatchObject({ appliedPatches: 3, skippedPatches: 0 });
    expect(reconstructor.getCurrentState()).toBe('hello world');
  });

  test('concatenates onto an own string field', () => {
    reconstructor.processSkeleton(skeleton({ s: 'ab', 'k.x': 'c' }));

    const result = reconstructor.applyPatch(
      patchFrame([
        { operation: 'append_str', path: '$.s', value: 'cd' },
        { operation: 'append_str', path: '$["k.x"]', value: 'd' }
      ])
    );

    expect(result).toMatchObject({ appliedPatches: 2, skippedPatches: 0 });
    expect(reconstructor.getCurrentState()).toEqual({ s: 'abcd', 'k.x': 'cd' });
  });

  test('concatenates onto a string array element', () => {
    reconstructor.processSkeleton(skeleton({ l: ['x', 'y'] }));

    reconstructor.applyPatch(patchFrame([{ operation: 'append_str', path: '$.l[1]', value: '!' }]));

    expect(reconstructor.getCurrentState()).toEqual({ l: ['x', 'y!'] });
  });

  test.each<[string, unknown, string]>([
    ['a number field', { v: 1 }, '$.v'],
    ['an array field', { v: [] }, '$.v'],
    ['a missing field', {}, '$.v'],
    ['an inherited property name', {}, '$.toString'],
    ['an object root', { v: 'x' }, '$'],
    ['an array root', ['x'], '$']
  ])('skips append_str on %s', (_label, root, path) => {
    reconstructor.processSkeleton(skeleton(root));

    const result = reconstructor.applyPatch(patchFrame([{ operation: 'append_str', path, value: 'z' }]));

    expect(result).toMatchObject({ appliedPatches: 0, skippedPatches: 1 });
    expect(reconstructor.getCurrentState()).toEqual(root);
  });
});
