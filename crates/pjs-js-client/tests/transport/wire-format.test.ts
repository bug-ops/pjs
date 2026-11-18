/**
 * @jest-environment node
 *
 * Wire details of the HTTP and SSE transports toward the shipped pjs-core
 * REST routes (#735): request order, bodies, headers and query parameters.
 * The end-to-end flow is covered by the shipped-routes suites.
 */

import { describe, test, expect, beforeEach, afterEach, jest } from '@jest/globals';
import { HttpTransport } from '../../src/transport/http.js';
import { SSETransport } from '../../src/transport/sse.js';
import { FrameType, TransportType } from '../../src/types/index.js';
import { makeConfig as baseConfig } from '../helpers/fake-pjs-server.js';

const STREAM_ID = 'stream-9';
const STREAMS = 'http://localhost:3000/pjs/sessions/session-1/streams';

const STREAM_OPTIONS = {
  sessionId: 'session-1',
  streamId: 'client-side-id',
  data: { a: 1 },
  queryParams: { limit: '5' },
  headers: { 'X-Test': 'yes' }
};

const FRAME = {
  frame_type: 'Skeleton',
  priority: 100,
  payload: {}
};

function makeConfig(transport: TransportType) {
  return { ...baseConfig(transport), headers: { Authorization: 'Bearer t' } };
}

let calls: Array<{ url: string; init: RequestInit }>;
let ndjson: string;
const originalFetch = globalThis.fetch;

beforeEach(() => {
  calls = [];
  ndjson = '';
  globalThis.fetch = jest.fn(async (url: unknown, init?: RequestInit) => {
    calls.push({ url: String(url), init: init ?? {} });
    if (init?.method === 'GET') {
      return new Response(ndjson, { status: 200, headers: { 'content-type': 'application/x-ndjson' } });
    }
    return new Response(JSON.stringify({ session_id: 'session-1', stream_id: STREAM_ID }), { status: 200 });
  }) as unknown as typeof fetch;
});

afterEach(() => {
  globalThis.fetch = originalFetch;
});

describe('HttpTransport wire format', () => {
  test('connect posts a string client_info to /pjs/sessions with the configured headers', async () => {
    const transport = new HttpTransport(makeConfig(TransportType.HTTP));

    const result = await transport.connect();

    expect(calls).toHaveLength(1);
    expect(calls[0]!.url).toBe('http://localhost:3000/pjs/sessions');
    expect(calls[0]!.init.method).toBe('POST');
    expect(calls[0]!.init.headers).toMatchObject({
      'Content-Type': 'application/json',
      Authorization: 'Bearer t'
    });
    expect(JSON.parse(calls[0]!.init.body as string)).toEqual({ client_info: expect.any(String) });
    expect(result).toEqual({ sessionId: 'session-1' });
  });

  test('startStream creates, starts and generates the stream, then reads its frames', async () => {
    const transport = new HttpTransport(makeConfig(TransportType.HTTP));
    await transport.connect();

    await transport.startStream('/ignored', STREAM_OPTIONS);

    const [create, start, generate, read] = calls.slice(1);
    expect(create!.url).toBe(STREAMS);
    expect(JSON.parse(create!.init.body as string)).toEqual({ data: { a: 1 } });
    expect(start!.url).toBe(`${STREAMS}/${STREAM_ID}/start`);
    expect(generate!.url).toBe(`${STREAMS}/${STREAM_ID}/generate-frames`);
    expect(JSON.parse(generate!.init.body as string)).toEqual({ priority_threshold: 10, max_frames: 1000 });
    for (const request of [create!, start!, generate!, read!]) {
      expect(request.init.headers).toMatchObject({ Authorization: 'Bearer t', 'X-Test': 'yes' });
    }

    const url = new URL(read!.url);
    expect(`${url.origin}${url.pathname}`).toBe(`${STREAMS}/${STREAM_ID}/frames/stream`);
    expect(Object.fromEntries(url.searchParams)).toEqual({ priority: '10', limit: '5' });
    expect(read!.init.method).toBe('GET');
    expect(read!.init.headers).toMatchObject({ Accept: 'application/x-ndjson' });
  });

  test('delivers each NDJSON line as a frame and completes the stream', async () => {
    ndjson = `${JSON.stringify(FRAME)}\n`;
    const transport = new HttpTransport(makeConfig(TransportType.HTTP));
    const frames: Array<{ frame_type: unknown }> = [];
    transport.on('frame', (frame: { frame_type: unknown }) => frames.push(frame));
    await transport.connect();

    await transport.startStream('/ignored', STREAM_OPTIONS);

    expect(frames[0]).toEqual(FRAME);
    expect(frames.map((frame) => frame.frame_type)).toEqual([FrameType.Skeleton, FrameType.Complete]);
  });

  test('an empty page still yields a Skeleton and a Complete frame', async () => {
    const transport = new HttpTransport(makeConfig(TransportType.HTTP));
    const frames: Array<{ frame_type: unknown; total_frames?: number }> = [];
    transport.on('frame', (frame: { frame_type: unknown }) => frames.push(frame));
    await transport.connect();

    await transport.startStream('/ignored', STREAM_OPTIONS);

    expect(frames.map((frame) => frame.frame_type)).toEqual([FrameType.Skeleton, FrameType.Complete]);
    expect(frames[1]!.total_frames).toBe(2);
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

  async function startedTransport(): Promise<{ transport: SSETransport; started: Promise<unknown> }> {
    const transport = new SSETransport(makeConfig(TransportType.ServerSentEvents));
    await transport.connect();
    const started = transport.startStream('/ignored', STREAM_OPTIONS).catch(() => undefined);
    await new Promise((resolve) => setImmediate(resolve));
    return { transport, started };
  }

  test('startStream opens an EventSource on frames/stream with the priority filter and query parameters', async () => {
    const { transport, started } = await startedTransport();

    const url = new URL(FakeEventSource.instances[0]!.url);
    expect(`${url.origin}${url.pathname}`).toBe(`${STREAMS}/${STREAM_ID}/frames/stream`);
    expect(Object.fromEntries(url.searchParams)).toEqual({ priority: '10', limit: '5' });
    await transport.disconnect();
    await started;
  });

  test('dispatches message and frame events as frames and error events as protocol errors', async () => {
    const { transport, started } = await startedTransport();
    const frames: unknown[] = [];
    const errors: Error[] = [];
    transport.on('frame', (frame: unknown) => frames.push(frame));
    transport.on('error', (error: Error) => errors.push(error));
    const source = FakeEventSource.instances[0]!;

    source.onmessage?.(new MessageEvent('message', { data: JSON.stringify(FRAME) }));
    source.emitNamed('frame', JSON.stringify({ ...FRAME, priority: 90 }));
    source.emitNamed('error', JSON.stringify({ message: 'boom' }));

    expect(frames).toEqual([FRAME, { ...FRAME, priority: 90 }]);
    expect(errors).toHaveLength(1);
    expect(errors[0]!.message).toBe('boom');
    await transport.disconnect();
    await started;
  });
});
