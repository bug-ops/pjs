/**
 * @jest-environment node
 *
 * Acceptance tests for the amended WebSocketTransport settle contract (#731):
 * a stream that has not completed always settles by rejecting, so
 * `PJSClient.processStream` unregisters its handler. Also pins the stale-socket
 * guard on messages, the malformed `Error` reply, empty patches before an
 * error, and trailing-slash base URLs.
 */

import { describe, test, expect, beforeEach, afterEach } from '@jest/globals';
import { PJSClient } from '../../src/core/client.js';
import { WebSocketTransport } from '../../src/transport/websocket.js';
import { PJSError, PJSErrorType, PJSEvent, TransportType } from '../../src/types/index.js';
import {
  FakeSocket,
  UUID_1,
  UUID_2,
  completeMessage,
  errorMessage,
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

async function connectTransport(baseUrl?: string): Promise<{ transport: WebSocketTransport; socket: FakeSocket }> {
  const transport = new WebSocketTransport(makeConfig(baseUrl));
  const connecting = transport.connect();
  const socket = FakeSocket.latest();
  socket.open();
  await connecting;
  return { transport, socket };
}

function startPending(transport: WebSocketTransport, data: Record<string, unknown> = {}): ReturnType<typeof settle> {
  const outcome = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: data as never }));
  return outcome;
}

function errorOf(result: Awaited<ReturnType<typeof settle>>): PJSError {
  expect(result.ok).toBe(false);
  return (result as { error: PJSError }).error;
}

describe('superseded stream through PJSClient', () => {
  test('the first stream rejects with StreamError and the second resolves with exactly its own document', async () => {
    const client = new PJSClient({ baseUrl: 'http://localhost:3000', transport: TransportType.WebSocket, timeout: 2000 });
    const connecting = client.connect();
    const socket = FakeSocket.latest();
    socket.open();
    await connecting;

    const first = settle(client.stream('/a', { data: { a: 1 } }));
    await flush();
    const second = client.stream('/b', { data: { b: 2 } });
    await flush();

    const firstError = errorOf(await first);
    expect(firstError.type).toBe(PJSErrorType.StreamError);

    const messages = socket.sentMessages();
    expect(messages.map((m) => m.type)).toEqual(['StreamInit', 'StreamCancel', 'StreamInit']);
    expect(messages[1].data).toEqual({ session_id: UUID_1 });
    expect(messages[2].data.session_id).toBe(UUID_2);

    socket.receive(patchMessage(0, [['set', ['stale'], true]], UUID_1));
    socket.receive(patchMessage(0, [['set', ['b'], 2]], UUID_2));
    socket.receive(completeMessage(UUID_2));

    expect(await second).toEqual({ b: 2 });
  });
});

describe('WebSocketTransport rejecting settle rules', () => {
  test('a second startStream rejects the first with StreamError "Stream superseded" before the new StreamInit', async () => {
    const { transport, socket } = await connectTransport();
    const first = startPending(transport, { n: 1 });
    await flush();

    void startPending(transport, { n: 2 });
    await flush();

    const error = errorOf(await first);
    expect(error.type).toBe(PJSErrorType.StreamError);
    expect(error.message).toBe('Stream superseded');
    expect(error.context).toMatchObject({ sessionId: UUID_1 });
    expect(socket.sentMessages().map((m) => m.type)).toEqual(['StreamInit', 'StreamCancel', 'StreamInit']);
  });

  test('stopStream rejects the pending stream with StreamError "Stream stopped" and sends StreamCancel', async () => {
    const { transport, socket } = await connectTransport();
    const outcome = startPending(transport);
    await flush();

    await transport.stopStream();

    const error = errorOf(await outcome);
    expect(error.type).toBe(PJSErrorType.StreamError);
    expect(error.message).toBe('Stream stopped');
    expect(error.context).toMatchObject({ sessionId: UUID_1 });
    expect(socket.sentMessages()[1]).toEqual({ type: 'StreamCancel', data: { session_id: UUID_1 } });
  });

  test('disconnect rejects the pending stream with ConnectionError "Client disconnected" and closes with 1000', async () => {
    const { transport, socket } = await connectTransport();
    const outcome = startPending(transport);
    await flush();

    await transport.disconnect();

    const error = errorOf(await outcome);
    expect(error.type).toBe(PJSErrorType.ConnectionError);
    expect(error.message).toBe('Client disconnected');
    expect(error.context).toMatchObject({ sessionId: UUID_1 });
    expect(socket.closeCalls[0].code).toBe(1000);
  });
});

describe('WebSocketTransport connect races', () => {
  test('disconnect while CONNECTING settles connect() and closes the socket', async () => {
    const transport = new WebSocketTransport(makeConfig());
    const connecting = settle(transport.connect());
    const socket = FakeSocket.latest();

    await transport.disconnect();

    expect(await settledOrPending(connecting)).not.toBe('pending');
    expect(socket.closeCalls.length).toBeGreaterThan(0);
    expect(transport.isTransportConnected()).toBe(false);
  });

  test('a second connect() while the first is CONNECTING closes the first socket and settles the first call', async () => {
    const transport = new WebSocketTransport(makeConfig());
    const first = settle(transport.connect());
    const firstSocket = FakeSocket.latest();

    const second = transport.connect();
    const secondSocket = FakeSocket.latest();
    secondSocket.open();

    await expect(second).resolves.toMatchObject({ supportedFeatures: ['patch-framing'] });
    expect(secondSocket).not.toBe(firstSocket);
    expect(firstSocket.closeCalls.length).toBeGreaterThan(0);
    expect(await settledOrPending(first)).not.toBe('pending');
    expect(transport.isTransportConnected()).toBe(true);
  });
});

describe('WebSocketTransport malformed Error reply', () => {
  test.each([
    ['code is a string', { session_id: UUID_1, error: 'x', code: '400' }],
    ['error is a number', { session_id: UUID_1, error: 5, code: 400 }],
    ['code is missing', { session_id: UUID_1, error: 'x' }],
    ['error is missing', { session_id: UUID_1, code: 400 }]
  ])('rejects ProtocolError "Malformed Error reply" and sends StreamCancel when %s', async (_label, data) => {
    const { transport, socket } = await connectTransport();
    const outcome = startPending(transport);
    await flush();

    socket.receive({ type: 'Error', data });

    const error = errorOf(await outcome);
    expect(error.type).toBe(PJSErrorType.ProtocolError);
    expect(error.message).toBe('Malformed Error reply');
    expect(error.context).toMatchObject({ sessionId: UUID_1 });
    expect(socket.sentMessages().map((m) => m.type)).toEqual(['StreamInit', 'StreamCancel']);
  });
});

describe('WebSocketTransport stale socket messages', () => {
  test('a message from the replaced socket with the live session id does not reach the active stream', async () => {
    const { transport, socket: stale } = await connectTransport();
    await transport.disconnect();

    const reconnecting = transport.connect();
    const current = FakeSocket.latest();
    current.open();
    await reconnecting;

    const frames: Array<{ frame_type: string; total_frames?: number }> = [];
    transport.on('frame', (frame: { frame_type: string; total_frames?: number }) => frames.push(frame));
    const outcome = startPending(transport);
    await flush();
    expect(current.sentMessages()[0].data.session_id).toBe(UUID_1);

    stale.receive(patchMessage(0, [['set', ['stale'], true]], UUID_1));
    stale.receive(completeMessage(UUID_1));
    stale.closeFromServer(1006);

    expect(frames).toEqual([]);
    expect(await settledOrPending(outcome)).toBe('pending');

    current.receive(patchMessage(0, [['set', ['live'], 1]], UUID_1));
    current.receive(completeMessage(UUID_1));

    expect(await outcome).toEqual({ ok: true });
    expect(frames.map((f) => f.frame_type)).toEqual(['Skeleton', 'Patch', 'Complete']);
    expect(frames[2].total_frames).toBe(3);
  });
});

describe('WebSocketTransport empty patches then Error', () => {
  test('emits no Skeleton frame and rejects with the server error', async () => {
    const { transport, socket } = await connectTransport();
    const frames: unknown[] = [];
    transport.on('frame', (frame: unknown) => frames.push(frame));
    const outcome = startPending(transport);
    await flush();

    socket.receive(patchMessage(0, [], UUID_1));
    socket.receive(errorMessage(UUID_1, 500, 'planner failed'));

    const error = errorOf(await outcome);
    expect(error.type).toBe(PJSErrorType.ProtocolError);
    expect(error.context).toEqual({ code: 500, error: 'planner failed' });
    expect(frames).toEqual([]);
  });
});

describe('WebSocketTransport base URL with a trailing slash', () => {
  test.each([
    ['http://localhost:3000/', 'ws://localhost:3000/ws'],
    ['https://example.com/', 'wss://example.com/ws'],
    ['http://localhost:3000/api/', 'ws://localhost:3000/api/ws']
  ])('opens %s as %s', async (baseUrl, expected) => {
    await connectTransport(baseUrl);

    expect(FakeSocket.latest().url).toBe(expected);
  });
});

describe('a stream started from a client listener', () => {
  async function connectedClient(): Promise<{ client: PJSClient; socket: FakeSocket }> {
    const client = new PJSClient({ baseUrl: 'http://localhost:3000', transport: TransportType.WebSocket, timeout: 500 });
    const connecting = client.connect();
    const socket = FakeSocket.latest();
    socket.open();
    await connecting;
    return { client, socket };
  }

  const outcomeOf = (promise: Promise<unknown>): Promise<{ ok: true; value: unknown } | { ok: false; error: unknown }> =>
    promise.then(
      (value) => ({ ok: true as const, value }),
      (error: unknown) => ({ ok: false as const, error })
    );

  test('from the StreamComplete listener: no StreamCancel, own frames, resolves with its own document', async () => {
    const { client, socket } = await connectedClient();
    let second: ReturnType<typeof outcomeOf> | undefined;
    client.once(PJSEvent.StreamComplete, () => {
      second = outcomeOf(client.stream('/x', { data: { doc: 'B' } }));
    });

    const first = client.stream('/x', { data: { doc: 'A' } });
    await flush();
    socket.receive(patchMessage(0, [['set', ['doc'], 'A']], UUID_1));
    socket.receive(completeMessage(UUID_1));
    expect(await first).toEqual({ doc: 'A' });
    await flush();

    expect(socket.sentMessages().map((m) => m.type)).toEqual(['StreamInit', 'StreamInit']);
    expect(socket.sentMessages()[1].data.session_id).toBe(UUID_2);

    socket.receive(patchMessage(0, [['set', ['doc'], 'B']], UUID_2));
    socket.receive(completeMessage(UUID_2));

    expect(await second).toEqual({ ok: true, value: { doc: 'B' } });
  });

  test('from the Patch listener: the old stream is cancelled once, the new one resolves with its own document', async () => {
    const { client, socket } = await connectedClient();
    let second: ReturnType<typeof outcomeOf> | undefined;
    client.once(PJSEvent.PatchApplied, () => {
      second = outcomeOf(client.stream('/x', { data: { doc: 'B' } }));
    });

    const first = outcomeOf(client.stream('/x', { data: { doc: 'A' } }));
    await flush();
    socket.receive(patchMessage(0, [['set', ['doc'], 'A']], UUID_1));
    await flush();

    const firstResult = await first;
    expect(firstResult.ok).toBe(false);
    expect((firstResult as { error: PJSError }).error.type).toBe(PJSErrorType.StreamError);
    expect(socket.sentMessages().map((m) => m.type)).toEqual(['StreamInit', 'StreamCancel', 'StreamInit']);

    socket.receive(completeMessage(UUID_1));
    socket.receive(patchMessage(0, [['set', ['doc'], 'B']], UUID_2));
    socket.receive(completeMessage(UUID_2));

    expect(await second).toEqual({ ok: true, value: { doc: 'B' } });
  });

  test('from the SkeletonReady listener of a zero-patch stream: its StreamComplete never completes the new stream', async () => {
    const { client, socket } = await connectedClient();
    let second: ReturnType<typeof outcomeOf> | undefined;
    client.once(PJSEvent.SkeletonReady, () => {
      second = outcomeOf(client.stream('/x', { data: { doc: 'B' } }));
    });

    const first = outcomeOf(client.stream('/x', { data: {} }));
    await flush();
    socket.receive(completeMessage(UUID_1));
    await flush();

    expect((await first).ok).toBe(false);
    expect(await settledOrPending(second as ReturnType<typeof outcomeOf>)).toBe('pending');

    socket.receive(patchMessage(0, [['set', ['doc'], 'B']], UUID_2));
    socket.receive(completeMessage(UUID_2));

    expect(await second).toEqual({ ok: true, value: { doc: 'B' } });
  });

  test("from the SkeletonReady listener: the old stream's first Patch frame never reaches the new stream", async () => {
    const { client, socket } = await connectedClient();
    let second: ReturnType<typeof outcomeOf> | undefined;
    client.once(PJSEvent.SkeletonReady, () => {
      second = outcomeOf(client.stream('/x', { data: { doc: 'B' } }));
    });

    const first = outcomeOf(client.stream('/x', { data: { doc: 'A' } }));
    await flush();
    socket.receive(patchMessage(0, [['set', ['doc'], 'A']], UUID_1));
    await flush();

    expect((await first).ok).toBe(false);
    expect(socket.sentMessages().map((m) => m.type)).toEqual(['StreamInit', 'StreamCancel', 'StreamInit']);

    socket.receive(patchMessage(0, [['set', ['doc'], 'B']], UUID_2));
    socket.receive(completeMessage(UUID_2));

    expect(await second).toEqual({ ok: true, value: { doc: 'B' } });
  });
});
