/**
 * Acceptance tests for #714: the wire format the experimental HTTP and SSE
 * transports speak toward an external server, as documented in the
 * README "Transport Options" table and spec 073 FR-011.
 */

import { describe, test, expect, beforeEach, afterEach, jest } from '@jest/globals';
import { HttpTransport } from '../../src/transport/http.js';
import { SSETransport } from '../../src/transport/sse.js';
import {
  Priority,
  TransportType,
  type PJSClientConfig
} from '../../src/types/index.js';

const STREAM_OPTIONS = {
  sessionId: 'session-1',
  streamId: 'stream-1',
  queryParams: { limit: '5' },
  headers: { 'X-Test': 'yes' }
};

const FRAME = {
  frame_type: 'skeleton',
  priority: 100,
  payload: { data: { a: 1 } }
};

function makeConfig(transport: TransportType): Required<PJSClientConfig> {
  return {
    baseUrl: 'http://localhost:3000',
    transport,
    sessionId: 'session-1',
    headers: { Authorization: 'Bearer t' },
    timeout: 1000,
    debug: false,
    bufferSize: 1024,
    priorityThreshold: Priority.Background,
    maxConcurrentStreams: 1
  };
}

function emptyNdjsonResponse(): Response {
  return {
    ok: true,
    status: 200,
    statusText: 'OK',
    headers: new Headers(),
    json: async () => ({ session_id: 'session-1', supported_features: ['x'] }),
    body: {
      getReader: () => ({
        read: async () => ({ done: true, value: undefined }),
        cancel: async () => {},
        releaseLock: () => {}
      })
    }
  } as unknown as Response;
}

describe('HttpTransport wire format', () => {
  const originalFetch = globalThis.fetch;
  let calls: Array<{ url: string; init: RequestInit }>;

  beforeEach(() => {
    calls = [];
    globalThis.fetch = jest.fn(async (url: unknown, init?: RequestInit) => {
      calls.push({ url: String(url), init: init ?? {} });
      return emptyNdjsonResponse();
    }) as unknown as typeof fetch;
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
  });

  test('connect posts a client_info object with a string version to /pjs/sessions', async () => {
    const transport = new HttpTransport(makeConfig(TransportType.HTTP));

    const result = await transport.connect();

    expect(calls).toHaveLength(1);
    expect(calls[0]!.url).toBe('http://localhost:3000/pjs/sessions');
    expect(calls[0]!.init.method).toBe('POST');
    expect(calls[0]!.init.headers).toMatchObject({
      'Content-Type': 'application/json',
      Authorization: 'Bearer t'
    });
    expect(JSON.parse(calls[0]!.init.body as string)).toEqual({
      client_info: { type: 'javascript', version: expect.any(String) }
    });
    expect(result).toEqual({ sessionId: 'session-1', supportedFeatures: ['x'] });
  });

  test('startStream gets /pjs/stream{endpoint} with ids, query params and NDJSON Accept', async () => {
    const transport = new HttpTransport(makeConfig(TransportType.HTTP));
    await transport.connect();

    await transport.startStream('/s1', STREAM_OPTIONS);

    const request = calls[1]!;
    const url = new URL(request.url);
    expect(`${url.origin}${url.pathname}`).toBe('http://localhost:3000/pjs/stream/s1');
    expect(Object.fromEntries(url.searchParams)).toEqual({
      session_id: 'session-1',
      stream_id: 'stream-1',
      limit: '5'
    });
    expect(request.init.method).toBe('GET');
    expect(request.init.headers).toMatchObject({
      Accept: 'application/x-ndjson',
      'X-Test': 'yes'
    });
  });

  test('delivers each NDJSON line as a frame', async () => {
    const encoder = new TextEncoder();
    const lines = encoder.encode(`${JSON.stringify(FRAME)}\n`);
    let sent = false;
    globalThis.fetch = jest.fn(async (_url: unknown, init?: RequestInit) =>
      init?.method === 'POST'
        ? emptyNdjsonResponse()
        : ({
            ok: true,
            status: 200,
            statusText: 'OK',
            headers: new Headers(),
            body: {
              getReader: () => ({
                read: async () => {
                  if (sent) return { done: true, value: undefined };
                  sent = true;
                  return { done: false, value: lines };
                },
                cancel: async () => {},
                releaseLock: () => {}
              })
            }
          } as unknown as Response)
    ) as unknown as typeof fetch;
    const transport = new HttpTransport(makeConfig(TransportType.HTTP));
    const frames: unknown[] = [];
    transport.on('frame', (frame: unknown) => frames.push(frame));
    await transport.connect();

    await transport.startStream('/s1', STREAM_OPTIONS);

    expect(frames).toEqual([FRAME]);
  });
});

class FakeEventSource extends EventTarget {
  static instances: FakeEventSource[] = [];
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;

  constructor(readonly url: string) {
    super();
    FakeEventSource.instances.push(this);
  }

  close(): void {}

  emitNamed(type: string, data: string): void {
    this.dispatchEvent(new MessageEvent(type, { data }));
  }
}

describe('SSETransport wire format', () => {
  const originalEventSource = globalThis.EventSource;

  beforeEach(() => {
    FakeEventSource.instances = [];
    (globalThis as { EventSource: unknown }).EventSource = FakeEventSource;
  });

  afterEach(() => {
    (globalThis as { EventSource: unknown }).EventSource = originalEventSource;
  });

  test('startStream opens GET /pjs/stream{endpoint}/sse with session and stream ids', async () => {
    const transport = new SSETransport(makeConfig(TransportType.ServerSentEvents));
    await transport.connect();

    await transport.startStream('/s1', STREAM_OPTIONS);

    const url = new URL(FakeEventSource.instances[0]!.url);
    expect(`${url.origin}${url.pathname}`).toBe('http://localhost:3000/pjs/stream/s1/sse');
    expect(Object.fromEntries(url.searchParams)).toEqual({
      session_id: 'session-1',
      stream_id: 'stream-1',
      limit: '5'
    });
  });

  test('dispatches message and frame events as frames and error events as protocol errors', async () => {
    const transport = new SSETransport(makeConfig(TransportType.ServerSentEvents));
    const frames: unknown[] = [];
    const errors: Error[] = [];
    transport.on('frame', (frame: unknown) => frames.push(frame));
    transport.on('error', (error: Error) => errors.push(error));
    await transport.connect();
    await transport.startStream('/s1', STREAM_OPTIONS);
    const source = FakeEventSource.instances[0]!;

    source.onmessage?.(new MessageEvent('message', { data: JSON.stringify(FRAME) }));
    source.emitNamed('frame', JSON.stringify(FRAME));
    source.emitNamed('error', JSON.stringify({ message: 'boom' }));

    expect(frames).toEqual([FRAME, FRAME]);
    expect(errors).toHaveLength(1);
    expect(errors[0]!.message).toBe('boom');
  });
});
