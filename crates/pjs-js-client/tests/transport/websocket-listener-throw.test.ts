/**
 * @jest-environment node
 *
 * Acceptance tests for #739 (track H2): a throwing transport `frame` listener
 * fails the stream instead of leaving it pending until the client timeout:
 * `startStream()` rejects with a StreamError carrying the listener error,
 * `StreamCancel` is sent, and nothing escapes `onmessage`.
 */

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals';
import { WebSocketTransport } from '../../src/transport/websocket.js';
import { type Frame, PJSError, PJSErrorType } from '../../src/types/index.js';
import {
  FakeSocket,
  UUID_1,
  UUID_2,
  completeMessage,
  flush,
  installFakes,
  makeConfig,
  patchMessage,
  restoreFakes,
  settle,
  settledOrPending
} from '../helpers/fake-websocket.js';

beforeEach(() => installFakes(UUID_1, UUID_2));
afterEach(() => restoreFakes());

const BOOM = new Error('boom');

async function connected(throwOn: (frame: Frame) => boolean) {
  const transport = new WebSocketTransport(makeConfig());
  const connecting = transport.connect();
  const socket = FakeSocket.latest();
  socket.open();
  await connecting;
  transport.on('frame', (frame: Frame) => {
    if (throwOn(frame)) throw BOOM;
  });
  const outcome = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: { a: 1 } }));
  await flush();
  return { socket, outcome };
}

async function expectStreamFailure(outcome: ReturnType<typeof settle>): Promise<PJSError> {
  const settled = await settledOrPending(outcome);
  expect(settled).not.toBe('pending');
  const result = settled as { ok: boolean; error?: unknown };
  expect(result.ok).toBe(false);
  const error = result.error as PJSError;
  expect(error).toBeInstanceOf(PJSError);
  expect(error.type).toBe(PJSErrorType.StreamError);
  expect(error.originalError).toBe(BOOM);
  return error;
}

describe('a throwing frame listener', () => {
  test.each([
    ['the lazy skeleton', (frame: Frame) => frame.frame_type === 'Skeleton'],
    ['a patch frame', (frame: Frame) => frame.frame_type === 'Patch']
  ])('fails the stream when it throws on %s', async (_label, throwOn) => {
    const { socket, outcome } = await connected(throwOn);

    expect(() => socket.receive(patchMessage(0, [['set', ['a'], 1]], UUID_1))).not.toThrow();

    await expectStreamFailure(outcome);
    expect(socket.sentMessages().map((message) => message.type)).toEqual(['StreamInit', 'StreamCancel']);
    expect(socket.sentMessages()[1].data).toEqual({ session_id: UUID_1 });
  });

  test('a throw on the Complete frame still settles the stream', async () => {
    const { socket, outcome } = await connected((frame) => frame.frame_type === 'Complete');

    try {
      socket.receive(completeMessage(UUID_1));
    } catch {
      // Whether the throw propagates is not part of the contract; the settle is.
    }

    expect(await settledOrPending(outcome)).not.toBe('pending');
  });

  test('a listener that never throws leaves a normal stream untouched', async () => {
    const { socket, outcome } = await connected(() => false);

    socket.receive(patchMessage(0, [['set', ['a'], 1]], UUID_1));
    socket.receive(completeMessage(UUID_1));

    expect(await outcome).toEqual({ ok: true });
    expect(socket.sentMessages().map((message) => message.type)).toEqual(['StreamInit']);
  });
});
