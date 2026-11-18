/**
 * @jest-environment node
 *
 * Acceptance tests for #763: a synchronous failure while starting a stream
 * (unserializable `StreamInit`, throwing `socket.send`) must leave no orphaned
 * active stream behind, so a later `startStream()` or `disconnect()` neither
 * sends a stray `StreamCancel` nor produces an unhandled rejection.
 */

import { describe, test, expect, beforeEach, afterEach } from '@jest/globals';
import { WebSocketTransport } from '../../src/transport/websocket.js';
import { PJSError, PJSErrorType } from '../../src/types/index.js';
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
  settle
} from '../helpers/fake-websocket.js';

let unhandled: unknown[] = [];
const onUnhandled = (reason: unknown): void => {
  unhandled.push(reason);
};

beforeEach(() => {
  installFakes(UUID_1, UUID_2);
  unhandled = [];
  process.on('unhandledRejection', onUnhandled);
});

afterEach(() => {
  process.off('unhandledRejection', onUnhandled);
  restoreFakes();
});

async function connectTransport(): Promise<{ transport: WebSocketTransport; socket: FakeSocket }> {
  const transport = new WebSocketTransport(makeConfig());
  const connecting = transport.connect();
  const socket = FakeSocket.latest();
  socket.open();
  await connecting;
  return { transport, socket };
}

function start(transport: WebSocketTransport, data: unknown): ReturnType<typeof settle> {
  return settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: data as never }));
}

async function settleMicrotasks(): Promise<void> {
  await flush();
  await flush();
}

const cyclic = (): Record<string, unknown> => {
  const doc: Record<string, unknown> = {};
  doc.self = doc;
  return doc;
};

describe.each([
  ['a BigInt value', (): unknown => ({ a: 10n })],
  ['a cyclic document', cyclic]
])('startStream with %s', (_label, makeDocument) => {
  test('a later startStream() streams normally and sends no StreamCancel for the failed one', async () => {
    const { transport, socket } = await connectTransport();
    await start(transport, makeDocument());

    const second = transport.startStream('/x', { sessionId: 's', streamId: 'st', data: { ok: 1 } as never });
    await flush();
    const init = socket.sentMessages()[0];
    socket.receive(completeMessage(init.data.session_id as string));
    await second;
    await settleMicrotasks();

    expect(socket.sentMessages().map((m) => m.type)).toEqual(['StreamInit']);
    expect(unhandled).toEqual([]);
  });

  test('rejects with the serialization error; disconnect() then produces no unhandled rejection and no StreamCancel', async () => {
    const { transport, socket } = await connectTransport();
    const outcome = await start(transport, makeDocument());
    expect(outcome.ok).toBe(false);
    expect((outcome as { error: unknown }).error).toBeInstanceOf(TypeError);

    await transport.disconnect();
    await settleMicrotasks();

    expect(socket.sent).toEqual([]);
    expect(unhandled).toEqual([]);
  });

  test('the previously active stream is left untouched', async () => {
    const { transport, socket } = await connectTransport();
    const first = start(transport, { first: true });
    await flush();

    const failed = await start(transport, makeDocument());
    expect(failed.ok).toBe(false);

    expect(socket.sentMessages().map((m) => m.type)).toEqual(['StreamInit']);
    socket.receive(patchMessage(0, [['set', ['first'], true]], UUID_1));
    socket.receive(completeMessage(UUID_1));
    expect((await first).ok).toBe(true);
    expect(unhandled).toEqual([]);
  });
});

describe('startStream when socket.send throws', () => {
  test('rejects with the send error and leaves no active stream', async () => {
    const { transport, socket } = await connectTransport();
    const sendError = new Error('send failed');
    const originalSend = socket.send.bind(socket);
    socket.send = (): void => {
      throw sendError;
    };

    const outcome = await start(transport, { a: 1 });
    expect(outcome.ok).toBe(false);
    expect((outcome as { error: unknown }).error).toBe(sendError);

    socket.send = originalSend;
    await transport.disconnect();
    await settleMicrotasks();

    expect(socket.sent).toEqual([]);
    expect(unhandled).toEqual([]);
  });

  test('the previous stream is already superseded with a StreamError and a following stream() works', async () => {
    const { transport, socket } = await connectTransport();
    const first = start(transport, { first: true });
    await flush();

    const originalSend = socket.send.bind(socket);
    let failNext = true;
    socket.send = (data: string): void => {
      if (failNext && JSON.parse(data).type === 'StreamInit') {
        failNext = false;
        throw new Error('send failed');
      }
      originalSend(data);
    };

    const failed = await start(transport, { second: true });
    expect(failed.ok).toBe(false);

    const firstOutcome = await first;
    expect(firstOutcome.ok).toBe(false);
    const firstError = (firstOutcome as { error: PJSError }).error;
    expect(firstError.type).toBe(PJSErrorType.StreamError);
    expect(socket.sentMessages().map((m) => m.type)).toEqual(['StreamInit', 'StreamCancel']);

    const third = transport.startStream('/x', { sessionId: 's', streamId: 'st', data: { third: true } as never });
    await flush();
    socket.receive(completeMessage(UUID_2));
    await third;
    await transport.disconnect();
    await settleMicrotasks();

    expect(unhandled).toEqual([]);
  });
});
