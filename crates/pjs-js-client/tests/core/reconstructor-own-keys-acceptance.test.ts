/**
 * Acceptance tests for #712 (keys named `__proto__`, `constructor`, `prototype`
 * are ordinary data) and #698 (quoted-key patch paths in the reconstructor).
 *
 * Every read goes through own properties and every write defines an own
 * property, so these keys round-trip and no patch can reach a prototype.
 */

import { describe, test, expect, beforeEach } from '@jest/globals';
import { JsonReconstructor } from '../../src/core/json-reconstructor.js';
import { FrameType, Priority, PatchFrame, SkeletonFrame } from '../../src/types/index.js';

type Op = 'set' | 'append' | 'merge' | 'delete';

function skeleton(json: string): SkeletonFrame {
  return {
    frame_type: FrameType.Skeleton,
    priority: Priority.Critical,
    payload: JSON.parse(json),
    complete: false,
    timestamp: 0
  } as SkeletonFrame;
}

function patchFrame(operation: Op, path: string, valueJson?: string): PatchFrame {
  const patch = {
    operation,
    path,
    ...(valueJson === undefined ? {} : { value: JSON.parse(valueJson) })
  };
  return {
    frame_type: FrameType.Patch,
    priority: Priority.High,
    payload: { patches: [patch] },
    timestamp: 0
  } as unknown as PatchFrame;
}

const DANGEROUS = ['__proto__', 'constructor', 'prototype'] as const;

function expectPrototypesUntouched(state: unknown): void {
  expect(({} as Record<string, unknown>).x).toBeUndefined();
  expect(({} as Record<string, unknown>).p).toBeUndefined();
  expect(Object.keys(Object.prototype)).toEqual([]);
  expect([Object.prototype, null]).toContain(Object.getPrototypeOf(state));
}

describe('JsonReconstructor own-property keys', () => {
  let reconstructor: JsonReconstructor;

  beforeEach(() => {
    reconstructor = new JsonReconstructor();
  });

  function apply(skeletonJson: string, operation: Op, path: string, valueJson?: string) {
    reconstructor.processSkeleton(skeleton(skeletonJson));
    const result = reconstructor.applyPatch(patchFrame(operation, path, valueJson));
    return { result, state: JSON.stringify(reconstructor.getCurrentState()) };
  }

  describe.each(DANGEROUS)('key %s', (name) => {
    test('set replaces the skeleton placeholder with the real value', () => {
      const { result, state } = apply(`{"${name}":null,"id":0}`, 'set', `$.${name}`, '"Acme"');

      expect(result.skippedPatches).toBe(0);
      expect(state).toBe(`{"${name}":"Acme","id":0}`);
    });

    test('set creates the key when the skeleton lacks it', () => {
      const { result, state } = apply('{"id":1}', 'set', `$.${name}`, '"v"');

      expect(result.skippedPatches).toBe(0);
      expect(state).toBe(`{"id":1,"${name}":"v"}`);
      expectPrototypesUntouched(reconstructor.getCurrentState());
    });

    test('append extends an own array and skips when the key is absent', () => {
      const own = apply(`{"${name}":[1]}`, 'append', `$.${name}`, '[2,3]');
      expect(own.state).toBe(`{"${name}":[1,2,3]}`);

      reconstructor.reset();
      const absent = apply('{}', 'append', `$.${name}`, '5');
      expect(absent.result.skippedPatches).toBe(1);
      expect(absent.state).toBe('{}');
      expectPrototypesUntouched(reconstructor.getCurrentState());
    });

    test('merge combines own objects and skips when the key is absent', () => {
      const own = apply(`{"${name}":{"a":1}}`, 'merge', `$.${name}`, '{"p":2}');
      expect(own.state).toBe(`{"${name}":{"a":1,"p":2}}`);

      reconstructor.reset();
      const absent = apply('{}', 'merge', `$.${name}`, '{"p":2}');
      expect(absent.result.skippedPatches).toBe(1);
      expect(absent.state).toBe('{}');
      expectPrototypesUntouched(reconstructor.getCurrentState());
    });

    test('delete removes only the own property', () => {
      const own = apply(`{"${name}":1,"id":2}`, 'delete', `$.${name}`);
      expect(own.state).toBe('{"id":2}');

      reconstructor.reset();
      const absent = apply('{"id":2}', 'delete', `$.${name}`);
      expect(absent.state).toBe('{"id":2}');
      expectPrototypesUntouched(reconstructor.getCurrentState());
    });

    test('nested set reaches into an own object of that name', () => {
      const { result, state } = apply(`{"${name}":{"x":0}}`, 'set', `$.${name}.x`, '1');

      expect(result.skippedPatches).toBe(0);
      expect(state).toBe(`{"${name}":{"x":1}}`);
      expectPrototypesUntouched(reconstructor.getCurrentState());
    });

    test('nested set through an absent key stays out of the prototype chain', () => {
      const { state } = apply('{}', 'set', `$.${name}.x`, '1');

      expect(state).toBe(`{"${name}":{"x":1}}`);
      expectPrototypesUntouched(reconstructor.getCurrentState());
    });
  });

  test('constructor.prototype chain round-trips as data', () => {
    const { result, state } = apply(
      '{"constructor":{"prototype":{"y":0}}}',
      'set',
      '$.constructor.prototype.y',
      '1'
    );

    expect(result.skippedPatches).toBe(0);
    expect(state).toBe('{"constructor":{"prototype":{"y":1}}}');
    expectPrototypesUntouched(reconstructor.getCurrentState());
  });

  test('constructor.prototype.x through an absent chain does not pollute Object.prototype', () => {
    const { state } = apply('{}', 'set', '$.constructor.prototype.x', '1');

    expect(state).toBe('{"constructor":{"prototype":{"x":1}}}');
    expectPrototypesUntouched(reconstructor.getCurrentState());
  });

  test('keys inside arrays and nested objects are kept', () => {
    const { result, state } = apply(
      '{"items":[{"constructor":null}],"company":{"prototype":null}}',
      'set',
      '$.items[0].constructor',
      '"x"'
    );
    expect(result.skippedPatches).toBe(0);
    expect(state).toBe('{"items":[{"constructor":"x"}],"company":{"prototype":null}}');

    const second = reconstructor.applyPatch(patchFrame('set', '$.company.prototype', '"v1"'));
    expect(second.skippedPatches).toBe(0);
    expect(JSON.stringify(reconstructor.getCurrentState())).toBe(
      '{"items":[{"constructor":"x"}],"company":{"prototype":"v1"}}'
    );
  });
});

describe('JsonReconstructor quoted-key patch paths', () => {
  let reconstructor: JsonReconstructor;

  beforeEach(() => {
    reconstructor = new JsonReconstructor();
  });

  test.each<[Op, string, string, string | undefined, string]>([
    ['set', '{"a.b":0,"":0,"x":{"k]":0}}', '$["a.b"]', '1', '{"a.b":1,"":0,"x":{"k]":0}}'],
    ['set', '{"a.b":0,"":0,"x":{"k]":0}}', '$[""]', '2', '{"a.b":0,"":2,"x":{"k]":0}}'],
    ['set', '{"a.b":0,"":0,"x":{"k]":0}}', '$.x["k]"]', '3', '{"a.b":0,"":0,"x":{"k]":3}}'],
    ['append', '{"a.b":[1]}', '$["a.b"]', '[2]', '{"a.b":[1,2]}'],
    ['merge', '{"a.b":{"c":1}}', '$["a.b"]', '{"d":2}', '{"a.b":{"c":1,"d":2}}'],
    ['delete', '{"a.b":1,"ok":2}', '$["a.b"]', undefined, '{"ok":2}'],
    ['set', '{"a.b":{"c":[0]}}', '$["a.b"].c[0]', '9', '{"a.b":{"c":[9]}}']
  ])('%s at %s', (operation, skeletonJson, path, valueJson, expected) => {
    reconstructor.processSkeleton(skeleton(skeletonJson));

    const result = reconstructor.applyPatch(patchFrame(operation, path, valueJson));

    expect(result.skippedPatches).toBe(0);
    expect(result.appliedPatches).toBe(1);
    expect(JSON.stringify(reconstructor.getCurrentState())).toBe(expected);
  });
});
