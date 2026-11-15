/**
 * A throwing user listener must not derail the shutdown flow (#644): no
 * ParseError, reader still cancelled, later listeners and the client's
 * ServerShutdown, Disconnected and pending-stream rejection still happen.
 */

import { describe, test, expect, afterEach, jest } from '@jest/globals';
import { PJSClient } from '../../src/core/client.js';
import { HttpTransport } from '../../src/transport/http.js';
import {
  PJSError,
  PJSErrorType,
  PJSEvent,
  Priority,
  TransportType,
  type PJSClientConfig
} from '../../src/types/index.js';

function makeConfig(): Required<PJSClientConfig> {
  return {
    baseUrl: 'http://localhost:3000',
    transport: TransportType.HTTP,
    sessionId: 'session-1',
    headers: {},
    timeout: 1000,
    debug: false,
    bufferSize: 1024,
    priorityThreshold: Priority.Background,
    maxConcurrentStreams: 1
  };
}

function ndjsonResponse(line: string, cancelled: { value: boolean }): Response {
  let sent = false;
  const reader = {
    read: async () => {
      if (cancelled.value || sent) return { done: true, value: undefined };
      sent = true;
      return { done: false, value: new TextEncoder().encode(line) };
    },
    cancel: async () => {
      cancelled.value = true;
    },
    releaseLock: () => {}
  };
  return {
    ok: true,
    status: 200,
    statusText: 'OK',
    headers: new Headers(),
    json: async () => ({ session_id: 'session-1' }),
    body: { getReader: () => reader }
  } as unknown as Response;
}

class RecordingEventSource extends EventTarget {
  static instances: RecordingEventSource[] = [];
  readyState = 1;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;

  constructor(readonly url: string) {
    super();
    RecordingEventSource.instances.push(this);
  }

  close(): void {
    this.readyState = 2;
  }
}

describe('shutdown with a throwing listener', () => {
  const originalFetch = globalThis.fetch;
  const originalEventSource = globalThis.EventSource;

  afterEach(() => {
    globalThis.fetch = originalFetch;
    (globalThis as { EventSource: unknown }).EventSource = originalEventSource;
  });

  test('NDJSON marker: no ParseError, reader cancelled, other listeners run', async () => {
    const cancelled = { value: false };
    globalThis.fetch = jest.fn(async (_url: unknown, init?: RequestInit) =>
      init?.method === 'POST'
        ? ndjsonResponse('', { value: false })
        : ndjsonResponse('{"event":"shutdown"}\n', cancelled)
    ) as unknown as typeof fetch;
    const transport = new HttpTransport(makeConfig());
    const errors: Error[] = [];
    let secondListenerCalls = 0;
    transport.on('error', (error: Error) => { errors.push(error); });
    transport.on('shutdown', () => { throw new Error('listener bug'); });
    transport.on('shutdown', () => { secondListenerCalls += 1; });

    await transport.connect();
    await transport.startStream('/s1', { sessionId: 'session-1', streamId: 'stream-1' });

    expect(cancelled.value).toBe(true);
    expect(secondListenerCalls).toBe(1);
    expect(errors.some((e) => e instanceof PJSError && e.type === PJSErrorType.ParseError)).toBe(false);
  });

  test('client: Disconnected and pending stream() rejection survive a throwing ServerShutdown listener', async () => {
    RecordingEventSource.instances = [];
    (globalThis as { EventSource: unknown }).EventSource = RecordingEventSource;
    const client = new PJSClient({
      baseUrl: 'http://localhost:3000',
      transport: TransportType.ServerSentEvents,
      timeout: 1000
    });
    const reasons: Array<string | undefined> = [];
    client.on(PJSEvent.ServerShutdown, () => { throw new Error('listener bug'); });
    client.on(PJSEvent.Disconnected, ({ reason }) => { reasons.push(reason); });

    const streaming = client.stream('/s1').catch((e: unknown) => e);
    await new Promise((resolve) => setTimeout(resolve, 0));
    RecordingEventSource.instances[0]!.dispatchEvent(
      new MessageEvent('shutdown', { data: '{"event":"shutdown"}' })
    );
    const error = await streaming;

    expect(reasons).toEqual(['server shutdown']);
    expect(error).toBeInstanceOf(PJSError);
    expect((error as PJSError).type).toBe(PJSErrorType.ServerShutdown);
  });
});

describe('emitShutdown listener semantics', () => {
  const originalFetch = globalThis.fetch;

  afterEach(() => {
    globalThis.fetch = originalFetch;
  });

  test('honors once() and binds this to the transport', async () => {
    globalThis.fetch = jest.fn(async (_url: unknown, init?: RequestInit) =>
      init?.method === 'POST'
        ? ndjsonResponse('', { value: false })
        : ndjsonResponse('{"event":"shutdown"}\n', { value: false })
    ) as unknown as typeof fetch;
    const transport = new HttpTransport(makeConfig());
    const receivers: unknown[] = [];
    transport.once('shutdown', function (this: unknown) { receivers.push(this); });

    await transport.connect();
    await transport.startStream('/s1', { sessionId: 'session-1', streamId: 'stream-1' });

    expect(receivers).toEqual([transport]);
    expect(transport.listenerCount('shutdown')).toBe(0);
  });

  test('logs a throwing listener under debug when no error listener exists', async () => {
    globalThis.fetch = jest.fn(async (_url: unknown, init?: RequestInit) =>
      init?.method === 'POST'
        ? ndjsonResponse('', { value: false })
        : ndjsonResponse('{"event":"shutdown"}\n', { value: false })
    ) as unknown as typeof fetch;
    const logged = jest.spyOn(console, 'error').mockImplementation(() => {});
    const thrown = new Error('listener bug');
    const transport = new HttpTransport({ ...makeConfig(), debug: true });
    transport.on('shutdown', () => { throw thrown; });

    await transport.connect();
    await transport.startStream('/s1', { sessionId: 'session-1', streamId: 'stream-1' });

    expect(logged).toHaveBeenCalledWith('[PJS] Shutdown listener threw:', thrown);
    logged.mockRestore();
  });
});
