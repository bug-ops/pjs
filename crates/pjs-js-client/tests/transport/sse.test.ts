/**
 * Acceptance tests for #644: SSETransport must deliver the named `shutdown`
 * event as a typed transport 'shutdown' notification, close the EventSource
 * and never reconnect.
 */

import { describe, test, expect, beforeEach, afterEach } from '@jest/globals';
import { SSETransport } from '../../src/transport/sse.js';
import {
  PJSError,
  Priority,
  TransportType,
  type PJSClientConfig
} from '../../src/types/index.js';

class RecordingEventSource extends EventTarget {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSED = 2;
  static instances: RecordingEventSource[] = [];

  readyState = RecordingEventSource.OPEN;
  closeCalls = 0;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;

  constructor(readonly url: string) {
    super();
    RecordingEventSource.instances.push(this);
  }

  close(): void {
    this.closeCalls += 1;
    this.readyState = RecordingEventSource.CLOSED;
  }

  emitNamed(type: string, data: string): void {
    this.dispatchEvent(new MessageEvent(type, { data }));
  }
}

function makeConfig(): Required<PJSClientConfig> {
  return {
    baseUrl: 'http://localhost:3000',
    transport: TransportType.ServerSentEvents,
    sessionId: 'session-1',
    headers: {},
    timeout: 1000,
    debug: false,
    bufferSize: 1024,
    priorityThreshold: Priority.Background,
    maxConcurrentStreams: 1
  };
}

describe('SSETransport shutdown event', () => {
  const originalEventSource = globalThis.EventSource;
  let transport: SSETransport;
  let source: RecordingEventSource;
  let shutdowns: number;
  let errors: Error[];
  let frames: unknown[];

  beforeEach(async () => {
    RecordingEventSource.instances = [];
    (globalThis as { EventSource: unknown }).EventSource = RecordingEventSource;

    transport = new SSETransport(makeConfig());
    shutdowns = 0;
    errors = [];
    frames = [];
    transport.on('shutdown', () => { shutdowns += 1; });
    transport.on('error', (error: Error) => { errors.push(error); });
    transport.on('frame', (frame: unknown) => { frames.push(frame); });

    await transport.connect();
    await transport.startStream('/s1', { sessionId: 'session-1', streamId: 'stream-1' });
    source = RecordingEventSource.instances[0]!;
  });

  afterEach(() => {
    (globalThis as { EventSource: unknown }).EventSource = originalEventSource;
  });

  test('emits shutdown once when the server sends the named event', () => {
    source.emitNamed('shutdown', '{"event":"shutdown"}');

    expect(shutdowns).toBe(1);
  });

  test('emits no error and no frame for the shutdown event', () => {
    source.emitNamed('shutdown', '{"event":"shutdown"}');

    expect(errors).toEqual([]);
    expect(frames).toEqual([]);
  });

  test('closes the EventSource so it cannot auto-reconnect', () => {
    source.emitNamed('shutdown', '{"event":"shutdown"}');

    expect(source.closeCalls).toBeGreaterThanOrEqual(1);
    expect(source.readyState).toBe(RecordingEventSource.CLOSED);
  });

  test('emits shutdown exactly once when the event is repeated', () => {
    source.emitNamed('shutdown', '{"event":"shutdown"}');
    source.emitNamed('shutdown', '{"event":"shutdown"}');

    expect(shutdowns).toBe(1);
  });

  test('keeps reporting plain connection errors before any shutdown', () => {
    source.onerror?.(new Event('error'));

    expect(errors).toHaveLength(1);
    expect(errors[0]).toBeInstanceOf(PJSError);
    expect(shutdowns).toBe(0);
  });

  test('still delivers regular frames before the shutdown event', () => {
    const frame = { frame_type: 'skeleton', priority: 100, payload: { data: {} } };
    source.onmessage?.(new MessageEvent('message', { data: JSON.stringify(frame) }));
    source.emitNamed('shutdown', '{"event":"shutdown"}');

    expect(frames).toEqual([frame]);
    expect(shutdowns).toBe(1);
  });
});
