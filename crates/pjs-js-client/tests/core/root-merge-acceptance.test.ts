/**
 * Acceptance tests for the in-place root `merge` (#670, JS half): same result
 * as the copying implementation, prototype-free state, `__proto__` stays data,
 * and no aliasing with the caller's value.
 */

import { describe, test, expect } from '@jest/globals';
import { JsonReconstructor } from '../../src/core/json-reconstructor.js';
import { FrameType, Priority, PatchFrame, SkeletonFrame } from '../../src/types/index.js';

function skeleton(payload: unknown): SkeletonFrame {
  return {
    frame_type: FrameType.Skeleton,
    priority: Priority.Critical,
    payload,
    complete: false,
    timestamp: 0
  } as SkeletonFrame;
}

function mergeFrame(value: unknown): PatchFrame {
  return {
    frame_type: FrameType.Patch,
    priority: Priority.High,
    payload: { patches: [{ operation: 'merge', path: '$', value }] },
    timestamp: 0
  } as unknown as PatchFrame;
}

function start(initial: unknown): JsonReconstructor {
  const reconstructor = new JsonReconstructor();
  reconstructor.processSkeleton(skeleton(initial));
  return reconstructor;
}

describe('JsonReconstructor root merge', () => {
  test('keeps the state prototype-free and __proto__ a plain data key', () => {
    const reconstructor = start({ a: 1 });
    const value = JSON.parse('{"__proto__": {"polluted": true}, "b": 2}');

    const result = reconstructor.applyPatch(mergeFrame(value));

    const state = reconstructor.getCurrentState() as Record<string, unknown>;
    expect(result.skippedPatches).toBe(0);
    expect(Object.getPrototypeOf(state)).toBeNull();
    expect(Object.keys(state).sort()).toEqual(['__proto__', 'a', 'b']);
    expect(Object.getOwnPropertyDescriptor(state, '__proto__')?.value).toEqual({ polluted: true });
    expect(({} as Record<string, unknown>).polluted).toBeUndefined();
  });

  test('later merges overwrite earlier keys and add new ones', () => {
    const reconstructor = start({ a: 1, keep: true });

    reconstructor.applyPatch(mergeFrame({ a: 2, b: { c: 3 } }));
    reconstructor.applyPatch(mergeFrame({ b: { d: 4 } }));

    expect(reconstructor.getCurrentState()).toEqual({ a: 2, keep: true, b: { d: 4 } });
  });

  test('does not alias the merged value or modify it', () => {
    const reconstructor = start({});
    const value = { nested: { n: 1 } };

    reconstructor.applyPatch(mergeFrame(value));
    reconstructor.applyPatch({
      frame_type: FrameType.Patch,
      priority: Priority.High,
      payload: { patches: [{ operation: 'set', path: '$.nested.n', value: 99 }] },
      timestamp: 0
    } as unknown as PatchFrame);

    expect(value).toEqual({ nested: { n: 1 } });
    expect(reconstructor.getCurrentState()).toEqual({ nested: { n: 99 } });
  });

  test('replaces the state when either side is not a plain object', () => {
    const arrayState = start([1]);
    arrayState.applyPatch(mergeFrame({ a: 1 }));
    const objectState = start({ a: 1 });
    objectState.applyPatch(mergeFrame([2]));

    expect(arrayState.getCurrentState()).toEqual({ a: 1 });
    expect(objectState.getCurrentState()).toEqual([2]);
  });
});
