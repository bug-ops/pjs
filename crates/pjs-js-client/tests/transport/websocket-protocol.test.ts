/**
 * @jest-environment node
 *
 * Acceptance tests for #731: `WebSocketTransport` speaks the shipped
 * `WsMessage` protocol on `/ws` (StreamInit/StreamPatch/StreamComplete/
 * StreamCancel/Error) with patch framing, against a scripted fake socket.
 */

import { describe, test, expect, beforeEach, afterEach, jest } from '@jest/globals';
import { PJSClient } from '../../src/core/client.js';
import { WebSocketTransport } from '../../src/transport/websocket.js';
import {
  PJSError,
  PJSErrorType,
  Priority,
  TransportType,
  type PJSClientConfig
} from '../../src/types/index.js';

const UUID = '11111111-1111-4111-8111-111111111111';
const OTHER_UUID = '22222222-2222-4222-8222-222222222222';
const SECOND_UUID = '33333333-3333-4333-8333-333333333333';

class FakeSocket {
  static instances: FakeSocket[] = [];
  static readonly OPEN = 1;

  readonly sent: string[] = [];
  readonly closeCalls: Array<{ code?: number; reason?: string }> = [];
  readyState = 0;
  onopen: ((event: unknown) => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  onclose: ((event: { code: number }) => void) | null = null;

  constructor(readonly url: string) {
    FakeSocket.instances.push(this);
  }

  send(data: string): void {
    this.sent.push(data);
  }

  close(code?: number, reason?: string): void {
    this.closeCalls.push({ code, reason });
  }

  open(): void {
    this.readyState = FakeSocket.OPEN;
    this.onopen?.({});
  }

  receive(message: unknown): void {
    this.onmessage?.({ data: typeof message === 'string' ? message : JSON.stringify(message) });
  }

  fail(): void {
    this.onerror?.({});
  }

  closeFromServer(code: number): void {
    this.onclose?.({ code });
  }

  sentMessages(): Array<{ type: string; data: Record<string, unknown> }> {
    return this.sent.map((text) => JSON.parse(text));
  }
}

function makeConfig(baseUrl = 'http://localhost:3000'): Required<PJSClientConfig> {
  return {
    baseUrl,
    transport: TransportType.WebSocket,
    sessionId: 'session-1',
    headers: {},
    timeout: 2000,
    debug: false,
    bufferSize: 1024,
    priorityThreshold: Priority.Background,
    maxConcurrentStreams: 1
  };
}

const flush = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));

type Outcome = { readonly ok: true } | { readonly ok: false; readonly error: unknown };

function settle(promise: Promise<void>): Promise<Outcome> {
  return promise.then(
    (): Outcome => ({ ok: true }),
    (error: unknown): Outcome => ({ ok: false, error })
  );
}

function patchMessage(
  frameId: number,
  patches: unknown[],
  sessionId = UUID,
  priority = 80,
  isComplete = false
): unknown {
  return {
    type: 'StreamPatch',
    data: { session_id: sessionId, frame_id: frameId, priority, patches, is_complete: isComplete }
  };
}

const completeMessage = (sessionId = UUID): unknown => ({
  type: 'StreamComplete',
  data: { session_id: sessionId }
});

const errorMessage = (sessionId: string | null, code: number, error = 'boom'): unknown => ({
  type: 'Error',
  data: { session_id: sessionId, error, code }
});

const originalWebSocket = Object.getOwnPropertyDescriptor(globalThis, 'WebSocket');
const originalCrypto = Object.getOwnPropertyDescriptor(globalThis, 'crypto');

function stubUuids(...ids: string[]): void {
  let next = 0;
  Object.defineProperty(globalThis, 'crypto', {
    value: { randomUUID: () => ids[Math.min(next++, ids.length - 1)] },
    configurable: true,
    writable: true
  });
}

function restoreGlobal(name: string, descriptor: PropertyDescriptor | undefined): void {
  if (descriptor) {
    Object.defineProperty(globalThis, name, descriptor);
  } else {
    Reflect.deleteProperty(globalThis, name);
  }
}

async function connectTransport(baseUrl?: string): Promise<{ transport: WebSocketTransport; socket: FakeSocket }> {
  const transport = new WebSocketTransport(makeConfig(baseUrl));
  const connecting = transport.connect();
  const socket = FakeSocket.instances[FakeSocket.instances.length - 1];
  socket.open();
  await connecting;
  return { transport, socket };
}

function collectFrames(transport: WebSocketTransport): unknown[] {
  const frames: unknown[] = [];
  transport.on('frame', (frame: unknown) => frames.push(frame));
  return frames;
}

beforeEach(() => {
  FakeSocket.instances = [];
  Object.defineProperty(globalThis, 'WebSocket', { value: FakeSocket, configurable: true, writable: true });
  stubUuids(UUID, SECOND_UUID);
});

afterEach(() => {
  jest.useRealTimers();
  restoreGlobal('WebSocket', originalWebSocket);
  restoreGlobal('crypto', originalCrypto);
});

describe('WebSocketTransport.connect', () => {
  test.each([
    ['http://localhost:3000', 'ws://localhost:3000/ws'],
    ['https://example.com', 'wss://example.com/ws'],
    ['ws://localhost:3000', 'ws://localhost:3000/ws'],
    ['wss://example.com', 'wss://example.com/ws'],
    ['http://localhost:3000/api/v1', 'ws://localhost:3000/api/v1/ws']
  ])('opens %s as %s', async (baseUrl, expected) => {
    await connectTransport(baseUrl);

    expect(FakeSocket.instances).toHaveLength(1);
    expect(FakeSocket.instances[0].url).toBe(expected);
  });

  test('resolves a session-less result advertising patch framing once open', async () => {
    const transport = new WebSocketTransport(makeConfig());
    const connecting = transport.connect();
    FakeSocket.instances[0].open();

    await expect(connecting).resolves.toEqual({
      sessionId: undefined,
      supportedFeatures: ['patch-framing']
    });
    expect(transport.isTransportConnected()).toBe(true);
  });

  test('rejects with ConnectionError when the socket errors before open', async () => {
    const transport = new WebSocketTransport(makeConfig());
    const connecting = transport.connect();
    FakeSocket.instances[0].fail();

    await expect(connecting).rejects.toMatchObject({ type: PJSErrorType.ConnectionError });
  });
});

describe('WebSocketTransport.startStream preconditions', () => {
  test('rejects with ConnectionError when not connected', async () => {
    const transport = new WebSocketTransport(makeConfig());

    await expect(
      transport.startStream('/x', { sessionId: 's', streamId: 'st', data: {} })
    ).rejects.toMatchObject({ type: PJSErrorType.ConnectionError });
  });

  test('rejects with ConfigurationError when data is missing and sends nothing', async () => {
    const { transport, socket } = await connectTransport();

    await expect(
      transport.startStream('/x', { sessionId: 's', streamId: 'st' })
    ).rejects.toMatchObject({ type: PJSErrorType.ConfigurationError });
    expect(socket.sent).toEqual([]);
  });

  test('accepts an explicit null document as data', async () => {
    const { transport, socket } = await connectTransport();

    void settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: null }));
    await flush();

    expect(socket.sentMessages()[0].data.data).toBeNull();
  });

  test('rejects with ConfigurationError when crypto.randomUUID is unavailable', async () => {
    const { transport, socket } = await connectTransport();
    Object.defineProperty(globalThis, 'crypto', { value: undefined, configurable: true, writable: true });

    await expect(
      transport.startStream('/x', { sessionId: 's', streamId: 'st', data: {} })
    ).rejects.toMatchObject({ type: PJSErrorType.ConfigurationError });
    expect(socket.sent).toEqual([]);
  });
});

describe('WebSocketTransport.startStream wire format', () => {
  test('sends one StreamInit with a fresh session id, the document and patch framing', async () => {
    const { transport, socket } = await connectTransport();

    void settle(transport.startStream('/ignored', { sessionId: 's', streamId: 'st', data: { a: [1, 'x'] } }));
    await flush();

    expect(socket.sent).toHaveLength(1);
    expect(JSON.parse(socket.sent[0])).toEqual({
      type: 'StreamInit',
      data: {
        session_id: UUID,
        data: { a: [1, 'x'] },
        options: { max_frame_size: 65536, framing: 'patch' }
      }
    });
  });

  test('ignores the endpoint argument', async () => {
    const { transport, socket } = await connectTransport();

    void settle(transport.startStream('/some/other/endpoint', { sessionId: 's', streamId: 'st', data: 1 }));
    await flush();

    expect(socket.sent[0]).not.toContain('/some/other/endpoint');
  });
});

describe('WebSocketTransport frame mapping', () => {
  test('maps StreamPatch to Skeleton, Patch and Complete frames and resolves', async () => {
    const { transport, socket } = await connectTransport();
    const frames = collectFrames(transport);
    const outcome = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: { a: 1 } }));
    await flush();

    socket.receive(patchMessage(0, [['set', ['a.b', 0, ''], 1], ['append', [], [2]]], UUID, 80));
    socket.receive(patchMessage(1, [['append_str', ['k'], 'tail']], UUID, 60));
    socket.receive(completeMessage());

    expect(await outcome).toEqual({ ok: true });
    expect(frames).toEqual([
      { frame_type: 'Skeleton', priority: 100, payload: {}, complete: false, stream_id: UUID },
      {
        frame_type: 'Patch',
        priority: 80,
        sequence: 0,
        stream_id: UUID,
        payload: {
          patches: [
            { operation: 'set', path: '$["a.b"][0][""]', value: 1 },
            { operation: 'append', path: '$', value: [2] }
          ]
        }
      },
      {
        frame_type: 'Patch',
        priority: 60,
        sequence: 1,
        stream_id: UUID,
        payload: { patches: [{ operation: 'append_str', path: '$.k', value: 'tail' }] }
      },
      { frame_type: 'Complete', priority: 100, stream_id: UUID, total_frames: 2 }
    ]);
  });

  test('a stream with zero patches still emits Skeleton then Complete', async () => {
    const { transport, socket } = await connectTransport();
    const frames = collectFrames(transport);
    const outcome = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: {} }));
    await flush();

    socket.receive(completeMessage());

    expect(await outcome).toEqual({ ok: true });
    expect(frames).toEqual([
      { frame_type: 'Skeleton', priority: 100, payload: {}, complete: false, stream_id: UUID },
      { frame_type: 'Complete', priority: 100, stream_id: UUID, total_frames: 0 }
    ]);
  });

  test('an empty patches array emits no frame and is not counted', async () => {
    const { transport, socket } = await connectTransport();
    const frames = collectFrames(transport);
    const outcome = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: {} }));
    await flush();

    socket.receive(patchMessage(0, []));
    socket.receive(patchMessage(1, [['set', ['a'], 1]]));
    socket.receive(completeMessage());
    await outcome;

    const types = frames.map((frame) => (frame as { frame_type: string }).frame_type);
    expect(types).toEqual(['Skeleton', 'Patch', 'Complete']);
    expect((frames[2] as { total_frames: number }).total_frames).toBe(1);
  });

  test('ignores unknown message types, other sessions and keeps going after unparseable text', async () => {
    const { transport, socket } = await connectTransport();
    const frames = collectFrames(transport);
    const errors: PJSError[] = [];
    transport.on('error', (error: PJSError) => errors.push(error));
    const outcome = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: {} }));
    await flush();

    socket.receive({ type: 'Pong', data: {} });
    socket.receive({ type: 'StreamFrame', data: {} });
    socket.receive(patchMessage(0, [['set', ['x'], 1]], OTHER_UUID));
    socket.receive(errorMessage(OTHER_UUID, 500));
    socket.receive(completeMessage(OTHER_UUID));
    socket.receive('not json {');
    socket.receive(completeMessage());

    expect(await outcome).toEqual({ ok: true });
    expect(frames).toHaveLength(2);
    expect(errors).toHaveLength(1);
    expect(errors[0].type).toBe(PJSErrorType.ParseError);
  });
});

describe('WebSocketTransport server errors', () => {
  test.each([400, 408, 409, 413, 429, 500])(
    'an Error with code %i for this session rejects with ProtocolError carrying code and text',
    async (code) => {
      const { transport, socket } = await connectTransport();
      const frames = collectFrames(transport);
      const outcome = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: {} }));
      await flush();

      socket.receive(errorMessage(UUID, code, 'server said no'));

      const result = await outcome;
      expect(result.ok).toBe(false);
      const error = (result as { error: PJSError }).error;
      expect(error.type).toBe(PJSErrorType.ProtocolError);
      expect(error.context).toEqual({ code, error: 'server said no' });
      expect(frames).toEqual([]);
    }
  );

  test('an Error with a null session id rejects the pending stream', async () => {
    const { transport, socket } = await connectTransport();
    const outcome = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: {} }));
    await flush();

    socket.receive(errorMessage(null, 400, 'malformed message'));

    const result = await outcome;
    expect(result.ok).toBe(false);
    expect((result as { error: PJSError }).error.type).toBe(PJSErrorType.ProtocolError);
    expect((result as { error: PJSError }).error.context).toEqual({ code: 400, error: 'malformed message' });
  });

  test('a frame_id gap rejects with ProtocolError and sends StreamCancel', async () => {
    const { transport, socket } = await connectTransport();
    const outcome = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: {} }));
    await flush();

    socket.receive(patchMessage(0, [['set', ['a'], 1]]));
    socket.receive(patchMessage(2, [['set', ['b'], 2]]));

    const result = await outcome;
    expect(result.ok).toBe(false);
    expect((result as { error: PJSError }).error.type).toBe(PJSErrorType.ProtocolError);
    expect(socket.sentMessages()[1]).toEqual({ type: 'StreamCancel', data: { session_id: UUID } });
  });

  test.each([
    ['patches is not an array', { session_id: UUID, frame_id: 0, priority: 80, patches: 'x', is_complete: false }],
    ['an operation is unknown', { session_id: UUID, frame_id: 0, priority: 80, patches: [['explode', [], 1]], is_complete: false }],
    ['a segment is a negative index', { session_id: UUID, frame_id: 0, priority: 80, patches: [['set', [-1], 1]], is_complete: false }],
    ['a segment is a fractional index', { session_id: UUID, frame_id: 0, priority: 80, patches: [['set', [1.5], 1]], is_complete: false }],
    ['frame_id is missing', { session_id: UUID, priority: 80, patches: [], is_complete: false }]
  ])('a malformed StreamPatch (%s) rejects with ProtocolError and sends StreamCancel', async (_label, data) => {
    const { transport, socket } = await connectTransport();
    const outcome = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: {} }));
    await flush();

    socket.receive({ type: 'StreamPatch', data });

    const result = await outcome;
    expect(result.ok).toBe(false);
    expect((result as { error: PJSError }).error.type).toBe(PJSErrorType.ProtocolError);
    expect(socket.sentMessages().map((m) => m.type)).toEqual(['StreamInit', 'StreamCancel']);
  });

  test('the socket closing before StreamComplete rejects with ConnectionError', async () => {
    const { transport, socket } = await connectTransport();
    const outcome = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: {} }));
    await flush();

    socket.closeFromServer(1006);

    const result = await outcome;
    expect(result.ok).toBe(false);
    expect((result as { error: PJSError }).error.type).toBe(PJSErrorType.ConnectionError);
  });
});

describe('WebSocketTransport lifecycle', () => {
  test('does not reconnect after an abnormal close', async () => {
    jest.useFakeTimers();
    const { transport, socket } = await connectTransport();
    let disconnects = 0;
    transport.on('disconnect', () => disconnects++);

    socket.closeFromServer(1006);
    await jest.advanceTimersByTimeAsync(120_000);

    expect(FakeSocket.instances).toHaveLength(1);
    expect(disconnects).toBe(1);
    expect(transport.isTransportConnected()).toBe(false);
  });

  test('stopStream sends StreamCancel for the active session and resolves the pending stream', async () => {
    const { transport, socket } = await connectTransport();
    const outcome = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: {} }));
    await flush();

    await transport.stopStream();

    expect(await outcome).toEqual({ ok: true });
    expect(socket.sentMessages()[1]).toEqual({ type: 'StreamCancel', data: { session_id: UUID } });
  });

  test('stopStream without an active stream sends nothing', async () => {
    const { transport, socket } = await connectTransport();

    await transport.stopStream();

    expect(socket.sent).toEqual([]);
  });

  test('disconnect closes with 1000 and resolves the pending stream without an error', async () => {
    const { transport, socket } = await connectTransport();
    const outcome = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: {} }));
    await flush();

    await transport.disconnect();
    socket.closeFromServer(1000);

    expect(await outcome).toEqual({ ok: true });
    expect(socket.closeCalls[0].code).toBe(1000);
    expect(transport.isTransportConnected()).toBe(false);
  });

  test('starting a second stream cancels the first and then sends its own StreamInit', async () => {
    const { transport, socket } = await connectTransport();
    const first = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: { n: 1 } }));
    await flush();

    const second = settle(transport.startStream('/x', { sessionId: 's', streamId: 'st', data: { n: 2 } }));
    await flush();

    expect(await first).toEqual({ ok: true });
    const messages = socket.sentMessages();
    expect(messages.map((m) => m.type)).toEqual(['StreamInit', 'StreamCancel', 'StreamInit']);
    expect(messages[1].data).toEqual({ session_id: UUID });
    expect(messages[2].data.session_id).toBe(SECOND_UUID);

    socket.receive(completeMessage(SECOND_UUID));
    expect(await second).toEqual({ ok: true });
  });
});

describe('WebSocketTransport stale socket events', () => {
  test('events from a replaced socket do not touch the current connection', async () => {
    const { transport, socket: stale } = await connectTransport();
    await transport.disconnect();

    const reconnecting = transport.connect();
    const current = FakeSocket.instances[FakeSocket.instances.length - 1];
    expect(current).not.toBe(stale);
    current.open();
    await reconnecting;

    const frames = collectFrames(transport);
    let disconnects = 0;
    transport.on('disconnect', () => disconnects++);

    stale.closeFromServer(1006);
    stale.receive(completeMessage());
    stale.fail();

    expect(disconnects).toBe(0);
    expect(frames).toEqual([]);
    expect(transport.isTransportConnected()).toBe(true);
  });

  test('a late open from a replaced socket does not mark the transport connected', async () => {
    const transport = new WebSocketTransport(makeConfig());
    void transport.connect().catch(() => undefined);
    const first = FakeSocket.instances[0];
    await transport.disconnect();

    first.open();

    expect(transport.isTransportConnected()).toBe(false);
  });
});

describe('PJSClient over the WebSocket transport', () => {
  async function connectedClient(): Promise<{ client: PJSClient; socket: FakeSocket }> {
    const client = new PJSClient({ baseUrl: 'http://localhost:3000', transport: TransportType.WebSocket, timeout: 2000 });
    const connecting = client.connect();
    const socket = FakeSocket.instances[FakeSocket.instances.length - 1];
    socket.open();
    await connecting;
    return { client, socket };
  }

  test('stream forwards options.data and reconstructs the streamed document', async () => {
    const { client, socket } = await connectedClient();
    const document = { a: 1, list: [1, 2, 3] };

    const result = client.stream('/ignored', { data: document });
    await flush();
    expect(socket.sentMessages()[0].data.data).toEqual(document);

    socket.receive(patchMessage(0, [['set', ['a'], 1], ['set', ['list'], []]]));
    socket.receive(patchMessage(1, [['append', ['list'], [1, 2, 3]]], UUID, 70));
    socket.receive(completeMessage());

    await expect(result).resolves.toEqual(document);
  });

  test('stream of a string root assembled from set and append_str across frames', async () => {
    const { client, socket } = await connectedClient();

    const result = client.stream('/ignored', { data: 'hello world' });
    await flush();

    socket.receive(patchMessage(0, [['set', [], '']]));
    socket.receive(patchMessage(1, [['append_str', [], 'hello ']], UUID, 70));
    socket.receive(patchMessage(2, [['append_str', [], 'world']], UUID, 60));
    socket.receive(completeMessage());

    await expect(result).resolves.toBe('hello world');
  });

  test('stream of an array root assembled from set and append across frames', async () => {
    const { client, socket } = await connectedClient();

    const result = client.stream('/ignored', { data: [1, 2, 3, 4] });
    await flush();

    socket.receive(patchMessage(0, [['set', [], []]]));
    socket.receive(patchMessage(1, [['append', [], [1, 2]]], UUID, 70));
    socket.receive(patchMessage(2, [['append', [], [3, 4]]], UUID, 60));
    socket.receive(completeMessage());

    await expect(result).resolves.toEqual([1, 2, 3, 4]);
  });

  test('a 413 reply surfaces as a ProtocolError with code 413', async () => {
    const { client, socket } = await connectedClient();

    const result = client.stream('/ignored', { data: {} });
    await flush();
    socket.receive(errorMessage(UUID, 413, 'message too large'));

    await expect(result).rejects.toMatchObject({
      type: PJSErrorType.ProtocolError,
      context: { code: 413 }
    });
  });
});
