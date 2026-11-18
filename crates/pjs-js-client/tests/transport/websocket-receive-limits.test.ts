/**
 * @jest-environment node
 *
 * Acceptance tests for #746 (track H2): the WebSocket transport enforces the
 * receiver-side limits spec 067 gives a client: a patch path longer than
 * `MAX_PATCH_PATH_DEPTH` (128) segments, and more than `maxStreamBytes`
 * cumulative UTF-8 bytes of `StreamPatch` messages in one stream, drop the
 * stream (ProtocolError, `StreamCancel` sent) before any frame is emitted.
 */

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals';
import { WebSocketTransport } from '../../src/transport/websocket.js';
import {
  type Frame,
  MAX_PATCH_PATH_DEPTH,
  PJSError,
  PJSErrorType
} from '../../src/types/index.js';
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

async function connected(maxStreamBytes?: number) {
  const config = maxStreamBytes === undefined ? makeConfig() : { ...makeConfig(), maxStreamBytes };
  const transport = new WebSocketTransport(config);
  const connecting = transport.connect();
  const socket = FakeSocket.latest();
  socket.open();
  await connecting;
  const frames: Frame[] = [];
  transport.on('frame', (frame: Frame) => frames.push(frame));
  return { transport, socket, frames };
}

function begin(transport: WebSocketTransport) {
  return settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: { a: 1 } }));
}

async function protocolErrorOf(outcome: ReturnType<typeof settle>): Promise<PJSError> {
  const settled = await settledOrPending(outcome);
  expect(settled).not.toBe('pending');
  expect((settled as { ok: boolean }).ok).toBe(false);
  const error = (settled as { error: unknown }).error as PJSError;
  expect(error).toBeInstanceOf(PJSError);
  expect(error.type).toBe(PJSErrorType.ProtocolError);
  return error;
}

describe('patch path depth', () => {
  test('MAX_PATCH_PATH_DEPTH is the protocol maximum of 128 segments', () => {
    expect(MAX_PATCH_PATH_DEPTH).toBe(128);
  });

  test('a path of exactly MAX_PATCH_PATH_DEPTH segments is accepted', async () => {
    const { transport, socket, frames } = await connected();
    const outcome = begin(transport);
    await flush();
    const path = Array.from({ length: MAX_PATCH_PATH_DEPTH }, () => 'a');

    socket.receive(patchMessage(0, [['set', path, 1]], UUID_1));
    socket.receive(completeMessage(UUID_1));

    expect(await outcome).toEqual({ ok: true });
    expect(frames.some((frame) => frame.frame_type === 'Patch')).toBe(true);
  });

  test('a longer path drops the stream with a ProtocolError and StreamCancel before any frame', async () => {
    const { transport, socket, frames } = await connected();
    const outcome = begin(transport);
    await flush();
    const path = Array.from({ length: MAX_PATCH_PATH_DEPTH + 1 }, () => 'a');

    socket.receive(patchMessage(0, [['set', path, 1]], UUID_1));

    await protocolErrorOf(outcome);
    expect(frames).toEqual([]);
    expect(socket.sentMessages().map((message) => message.type)).toEqual(['StreamInit', 'StreamCancel']);
  });
});

describe('cumulative stream bytes', () => {
  /** A patch message whose JSON has more UTF-8 bytes than characters. */
  function wideMessage(frameId: number, sessionId: string): unknown {
    return patchMessage(frameId, [['set', ['a'], 'ж'.repeat(300)]], sessionId);
  }

  test('the cap counts UTF-8 bytes, not characters', () => {
    const text = JSON.stringify(wideMessage(0, UUID_1));

    expect(Buffer.byteLength(text)).toBeGreaterThan(500);
    expect(Buffer.byteLength(text)).toBeLessThan(1000);
    expect(2 * text.length).toBeLessThan(1000);
  });

  test('a message that pushes the stream past maxStreamBytes drops it before its frame is emitted', async () => {
    const { transport, socket, frames } = await connected(1000);
    const outcome = begin(transport);
    await flush();

    socket.receive(wideMessage(0, UUID_1));
    const framesAfterFirst = frames.length;
    socket.receive(wideMessage(1, UUID_1));

    await protocolErrorOf(outcome);
    expect(framesAfterFirst).toBeGreaterThan(0);
    expect(frames).toHaveLength(framesAfterFirst);
    expect(socket.sentMessages().map((message) => message.type)).toEqual(['StreamInit', 'StreamCancel']);
  });

  test('the byte count starts again with every stream', async () => {
    const { transport, socket } = await connected(1000);

    const first = begin(transport);
    await flush();
    socket.receive(wideMessage(0, UUID_1));
    socket.receive(completeMessage(UUID_1));
    expect(await first).toEqual({ ok: true });

    const second = begin(transport);
    await flush();
    socket.receive(wideMessage(0, UUID_2));
    socket.receive(completeMessage(UUID_2));
    expect(await second).toEqual({ ok: true });
  });
});
