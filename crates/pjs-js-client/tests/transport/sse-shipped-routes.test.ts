/**
 * @jest-environment node
 *
 * Acceptance tests for #735 (track H1): SSETransport creates the session and
 * the stream through the shipped REST routes, then opens an EventSource on
 * `frames/stream` (the server answers `text/event-stream` to an EventSource's
 * default Accept). It emits each `data:` frame, sets `total_frames` on the
 * Complete frame, and rejects a superseded stream with STREAM_ERROR.
 */

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals';
import { SSETransport } from '../../src/transport/sse.js';
import { type Frame, PJSError, PJSErrorType, TransportType } from '../../src/types/index.js';
import {
  COMPLETE_FRAME,
  DOCUMENT,
  FakePjsServer,
  PATCH_FRAME,
  SESSION_ID,
  SKELETON_FRAME,
  makeConfig
} from '../helpers/fake-pjs-server.js';

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  readonly listeners = new Map<string, Array<(event: unknown) => void>>();
  onopen: ((event: unknown) => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  closed = false;

  constructor(readonly url: string) {
    FakeEventSource.instances.push(this);
  }

  addEventListener(type: string, listener: (event: unknown) => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  close(): void {
    this.closed = true;
  }

  deliver(frame: unknown): void {
    this.onmessage?.({ data: JSON.stringify(frame) });
  }
}

const flush = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));
const originalEventSource = Object.getOwnPropertyDescriptor(globalThis, 'EventSource');
let server: FakePjsServer;

beforeEach(() => {
  FakeEventSource.instances = [];
  Object.defineProperty(globalThis, 'EventSource', {
    value: FakeEventSource,
    configurable: true,
    writable: true
  });
  server = new FakePjsServer([]);
  server.install();
});

afterEach(() => {
  server.restore();
  if (originalEventSource) {
    Object.defineProperty(globalThis, 'EventSource', originalEventSource);
  } else {
    Reflect.deleteProperty(globalThis, 'EventSource');
  }
});

async function startedTransport(): Promise<{ transport: SSETransport; started: Promise<unknown> }> {
  const transport = new SSETransport(makeConfig(TransportType.ServerSentEvents));
  await transport.connect();
  const started = transport
    .startStream('/ignored', { sessionId: SESSION_ID, streamId: 'client-side-id', data: DOCUMENT })
    .then(
      () => undefined,
      (error: unknown) => error
    );
  await flush();
  await flush();
  return { transport, started };
}

describe('SSETransport over the shipped routes', () => {
  test('connect creates a session with a string client_info', async () => {
    const transport = new SSETransport(makeConfig(TransportType.ServerSentEvents));

    const connected = await transport.connect();

    const [request] = server.requestsTo('POST', '/pjs/sessions');
    expect(typeof (request.body as { client_info: unknown }).client_info).toBe('string');
    expect(connected.sessionId).toBe(SESSION_ID);
  });

  test('startStream creates the stream with the document and opens an EventSource on frames/stream', async () => {
    await startedTransport();

    const [created] = server.requestsTo('POST', `/pjs/sessions/${SESSION_ID}/streams`);
    expect(created.body).toEqual({ data: DOCUMENT });
    expect(FakeEventSource.instances).toHaveLength(1);
    expect(new URL(FakeEventSource.instances[0].url).pathname).toBe(new URL(server.streamUrl()).pathname);
    expect(server.requests.filter((r) => r.path.startsWith('/pjs/stream/'))).toEqual([]);
  });

  test('every data: frame is emitted and the Complete frame carries total_frames', async () => {
    const { transport } = await startedTransport();
    const frames: Frame[] = [];
    transport.on('frame', (frame: Frame) => frames.push(frame));
    const source = FakeEventSource.instances[0];

    source.deliver(SKELETON_FRAME);
    source.deliver(PATCH_FRAME);
    source.deliver(COMPLETE_FRAME);

    expect(frames.map((frame) => frame.frame_type)).toEqual(['Skeleton', 'Patch', 'Complete']);
    expect((frames[2] as { total_frames?: number }).total_frames).toBe(3);
  });

  test('a second startStream rejects the first with STREAM_ERROR and closes its EventSource', async () => {
    const { transport, started } = await startedTransport();

    const second = transport
      .startStream('/b', { sessionId: SESSION_ID, streamId: 'b', data: { b: 2 } })
      .then(
        () => undefined,
        (error: unknown) => error
      );
    await flush();
    await flush();

    const error = await Promise.race([started, flush().then(() => 'still pending')]);
    expect(error).toBeInstanceOf(PJSError);
    expect((error as PJSError).type).toBe(PJSErrorType.StreamError);
    expect(FakeEventSource.instances[0].closed).toBe(true);
    await transport.disconnect();
    await second;
  });
});
