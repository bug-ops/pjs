/**
 * Acceptance tests for #731 follow-ups: `append_str` with a non-string value is
 * skipped (root and field), and an array append of a very large value does not
 * overflow the call stack.
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

const NON_STRINGS: Array<[string, unknown]> = [
  ['a number', 5],
  ['null', null],
  ['an object', { a: 1 }],
  ['an array', ['a']]
];

describe('JsonReconstructor append_str with a non-string value', () => {
  test.each(NON_STRINGS)('skips a root append_str of %s', (_label, value) => {
    const reconstructor = new JsonReconstructor();
    reconstructor.processSkeleton(skeleton('abc'));

    const result = reconstructor.applyPatch(patchFrame([{ operation: 'append_str', path: '$', value }]));

    expect(result).toMatchObject({ appliedPatches: 0, skippedPatches: 1 });
    expect(reconstructor.getCurrentState()).toBe('abc');
  });

  test.each(NON_STRINGS)('skips a field append_str of %s', (_label, value) => {
    const reconstructor = new JsonReconstructor();
    reconstructor.processSkeleton(skeleton({ s: 'abc' }));

    const result = reconstructor.applyPatch(patchFrame([{ operation: 'append_str', path: '$.s', value }]));

    expect(result).toMatchObject({ appliedPatches: 0, skippedPatches: 1 });
    expect(reconstructor.getCurrentState()).toEqual({ s: 'abc' });
  });
});

describe('JsonReconstructor append of a very large array', () => {
  const LARGE = 200_000;

  test('a root append of a 200k-element array applies without throwing', () => {
    const reconstructor = new JsonReconstructor();
    reconstructor.processSkeleton(skeleton([]));
    const elements = Array.from({ length: LARGE }, (_, i) => i);

    const result = reconstructor.applyPatch(patchFrame([{ operation: 'append', path: '$', value: elements }]));

    expect(result).toMatchObject({ appliedPatches: 1, skippedPatches: 0 });
    const state = reconstructor.getCurrentState() as number[];
    expect(state).toHaveLength(LARGE);
    expect(state[LARGE - 1]).toBe(LARGE - 1);
  });

  test('a field append of a 200k-element array applies without throwing', () => {
    const reconstructor = new JsonReconstructor();
    reconstructor.processSkeleton(skeleton({ list: [] }));
    const elements = Array.from({ length: LARGE }, (_, i) => i);

    const result = reconstructor.applyPatch(patchFrame([{ operation: 'append', path: '$.list', value: elements }]));

    expect(result).toMatchObject({ appliedPatches: 1, skippedPatches: 0 });
    expect((reconstructor.getCurrentState() as { list: number[] }).list).toHaveLength(LARGE);
  });
});
