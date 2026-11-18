/**
 * @jest-environment node
 *
 * Acceptance tests for #746 (track H2): `stateDelivery` picks how the
 * per-frame state reaches `onRender`. `'live'` (the default) hands over the
 * reconstructor's own state without a deep clone per frame (documented
 * read-only); `'snapshot'` hands over an independent clone each time. The
 * result of `stream()` is a clone made once.
 */

import { afterEach, beforeEach, describe, expect, jest, test } from '@jest/globals';
import { PJSClient } from '../../src/core/client.js';
import { JsonReconstructor } from '../../src/core/json-reconstructor.js';
import { TransportType } from '../../src/types/index.js';
import {
  FakeSocket,
  UUID_1,
  completeMessage,
  flush,
  installFakes,
  patchMessage,
  restoreFakes
} from '../helpers/fake-websocket.js';

beforeEach(() => installFakes(UUID_1));
afterEach(() => {
  restoreFakes();
  jest.restoreAllMocks();
});

type Delivery = 'live' | 'snapshot';

interface Run {
  renders: unknown[];
  result: unknown;
}

async function streamThreePatches(stateDelivery?: Delivery): Promise<Run> {
  const client = new PJSClient({
    baseUrl: 'http://localhost:3000',
    transport: TransportType.WebSocket,
    timeout: 2000,
    ...(stateDelivery === undefined ? {} : { stateDelivery })
  });
  const connecting = client.connect();
  const socket = FakeSocket.latest();
  socket.open();
  await connecting;

  const renders: unknown[] = [];
  const pending = client.stream('/x', {
    data: { a: 1, b: 2, c: 3 },
    onRender: (data) => {
      renders.push(data);
    }
  });
  await flush();
  socket.receive(patchMessage(0, [['set', ['a'], 1]], UUID_1));
  socket.receive(patchMessage(1, [['set', ['b'], 2]], UUID_1));
  socket.receive(patchMessage(2, [['set', ['c'], 3]], UUID_1));
  socket.receive(completeMessage(UUID_1));
  const result = await pending;
  await client.disconnect();
  return { renders, result };
}

describe("stateDelivery 'live' (the default)", () => {
  test.each<[string, Delivery | undefined]>([
    ['the default', undefined],
    ["explicit 'live'", 'live']
  ])('%s hands every patch render the same state object', async (_label, delivery) => {
    const { renders, result } = await streamThreePatches(delivery);

    // renders: skeleton, three patches, complete
    expect(renders).toHaveLength(5);
    expect(renders[1]).toBe(renders[2]);
    expect(renders[2]).toBe(renders[3]);
    expect(renders[3]).toEqual({ a: 1, b: 2, c: 3 });
    expect(result).toEqual({ a: 1, b: 2, c: 3 });
  });

  test('the stream() result is a clone, not the live state', async () => {
    const { renders, result } = await streamThreePatches('live');

    expect(result).not.toBe(renders[3]);
  });

  test('the state is not deep-cloned once per patch frame', async () => {
    const spy = jest.spyOn(JsonReconstructor.prototype, 'getCurrentState');

    await streamThreePatches('live');

    expect(spy.mock.calls.length).toBeLessThanOrEqual(1);
  });
});

describe("stateDelivery 'snapshot'", () => {
  test('every render gets its own copy that later patches do not change', async () => {
    const { renders, result } = await streamThreePatches('snapshot');

    expect(renders[1]).toEqual({ a: 1 });
    expect(renders[2]).toEqual({ a: 1, b: 2 });
    expect(renders[3]).toEqual({ a: 1, b: 2, c: 3 });
    expect(renders[1]).not.toBe(renders[2]);
    expect(result).toEqual({ a: 1, b: 2, c: 3 });
  });

  test('the state is cloned for each patch frame', async () => {
    const spy = jest.spyOn(JsonReconstructor.prototype, 'getCurrentState');

    await streamThreePatches('snapshot');

    expect(spy.mock.calls.length).toBeGreaterThanOrEqual(3);
  });
});
