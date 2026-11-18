/**
 * Acceptance tests for #764 (track H2): `append` needs an existing array and
 * `merge` an existing object at the target, as in spec 067 and the Rust
 * client's `apply_patch`. A patch whose target is missing or of the wrong kind
 * is counted in `skippedPatches` and leaves the state unchanged; nothing is
 * replaced or created for it.
 */

import { describe, test, expect } from '@jest/globals';
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

function run(base: unknown, operation: Op, path: string, value: unknown) {
  const reconstructor = new JsonReconstructor();
  reconstructor.processSkeleton(skeleton(base));
  const result = reconstructor.applyPatch(patchFrame([{ operation, path, value }]));
  return { result, state: reconstructor.getCurrentState() as unknown };
}

describe('JsonReconstructor append target kinds', () => {
  const REJECTED: Array<[string, unknown, string]> = [
    ['an object', { b: { c: 1 } }, '$.b'],
    ['a string', { b: 'keep' }, '$.b'],
    ['a number', { b: 42 }, '$.b'],
    ['null', { b: null }, '$.b'],
    ['a missing key', { other: 1 }, '$.b'],
    ['a root object', { b: 1 }, '$'],
    ['a root scalar', 7, '$']
  ];

  test.each(REJECTED)('skips an append onto %s and keeps the state', (_label, base, path) => {
    const before = JSON.parse(JSON.stringify(base));

    const { result, state } = run(base, 'append', path, [1]);

    expect(result).toMatchObject({ appliedPatches: 0, skippedPatches: 1 });
    expect(state).toEqual(before);
  });

  test('appends a scalar and spreads an array onto an existing array field', () => {
    const scalar = run({ b: [1] }, 'append', '$.b', 2);
    const spread = run({ b: [1] }, 'append', '$.b', [2, 3]);

    expect(scalar.state).toEqual({ b: [1, 2] });
    expect(spread.state).toEqual({ b: [1, 2, 3] });
    expect(scalar.result).toMatchObject({ appliedPatches: 1, skippedPatches: 0 });
  });

  test('appends onto an existing root array', () => {
    expect(run([1], 'append', '$', [2]).state).toEqual([1, 2]);
  });
});

describe('JsonReconstructor merge target kinds', () => {
  const REJECTED: Array<[string, unknown, string]> = [
    ['a root scalar', 1, '$'],
    ['a root array', [1], '$'],
    ['a string field', { b: 'x' }, '$.b'],
    ['an array field', { b: [1] }, '$.b'],
    ['a missing key', { other: 1 }, '$.b']
  ];

  test.each(REJECTED)('skips a merge onto %s and keeps the state', (_label, base, path) => {
    const before = JSON.parse(JSON.stringify(base));

    const { result, state } = run(base, 'merge', path, { m: 1 });

    expect(result).toMatchObject({ appliedPatches: 0, skippedPatches: 1 });
    expect(state).toEqual(before);
  });

  test('merges entries into an existing object field and the root object', () => {
    expect(run({ b: { a: 1 } }, 'merge', '$.b', { m: 2 }).state).toEqual({ b: { a: 1, m: 2 } });
    expect(run({ a: 1 }, 'merge', '$', { m: 2 }).state).toEqual({ a: 1, m: 2 });
  });
});

describe('JsonReconstructor set keeps creating intermediate objects', () => {
  test('a set under missing keys builds the path', () => {
    const { result, state } = run({}, 'set', '$.x.y', 1);

    expect(state).toEqual({ x: { y: 1 } });
    expect(result).toMatchObject({ appliedPatches: 1, skippedPatches: 0 });
  });
});
