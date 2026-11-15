/**
 * Acceptance tests for #644 (NDJSON sibling): the server ends an NDJSON stream
 * with the terminal line `{"event":"shutdown"}`. HttpTransport must report it
 * as a 'shutdown' notification, not as a frame or a validation error, and stop
 * reading.
 */

import { TextDecoder, TextEncoder } from 'node:util';
import { describe, test, expect, beforeEach, afterEach, jest } from '@jest/globals';
import { HttpTransport } from '../../src/transport/http.js';
import {
  Priority,
  TransportType,
  type PJSClientConfig
} from '../../src/types/index.js';

// jsdom does not expose the Node text codecs the transport relies on.
Object.assign(globalThis, { TextDecoder, TextEncoder });

const SKELETON_LINE = JSON.stringify({
  frame_type: 'skeleton',
  priority: 100,
  payload: { data: { a: 1 } }
});
const MARKER_LINE = '{"event":"shutdown"}';

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

interface ReaderProbe {
  reads: number;
  cancelled: boolean;
}

function ndjsonResponse(chunks: string[], probe: ReaderProbe): Response {
  const encoder = new TextEncoder();
  let index = 0;
  const reader = {
    read: async () => {
      probe.reads += 1;
      if (probe.cancelled || index >= chunks.length) {
        return { done: true, value: undefined };
      }
      return { done: false, value: encoder.encode(chunks[index++]) };
    },
    cancel: async () => {
      probe.cancelled = true;
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

describe('HttpTransport NDJSON shutdown marker', () => {
  const originalFetch = globalThis.fetch;
  let transport: HttpTransport;
  let probe: ReaderProbe;
  let shutdowns: number;
  let errors: Error[];
  let frames: unknown[];

  async function runStream(chunks: string[]): Promise<void> {
    globalThis.fetch = jest.fn(async (_url: unknown, init?: RequestInit) =>
      init?.method === 'POST'
        ? ndjsonResponse([], { reads: 0, cancelled: false })
        : ndjsonResponse(chunks, probe)
    ) as unknown as typeof fetch;
    await transport.connect();
    await transport.startStream('/s1', { sessionId: 'session-1', streamId: 'stream-1' });
  }

  beforeEach(() => {
    transport = new HttpTransport(makeConfig());
    probe = { reads: 0, cancelled: false };
    shutdowns = 0;
    errors = [];
    frames = [];
    transport.on('shutdown', () => { shutdowns += 1; });
    transport.on('error', (error: Error) => { errors.push(error); });
    transport.on('frame', (frame: unknown) => { frames.push(frame); });
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
  });

  test('emits shutdown once for the terminal marker line', async () => {
    await runStream([`${SKELETON_LINE}\n${MARKER_LINE}\n`]);

    expect(shutdowns).toBe(1);
  });

  test('emits neither a frame nor an error for the marker line', async () => {
    await runStream([`${MARKER_LINE}\n`]);

    expect(frames).toEqual([]);
    expect(errors).toEqual([]);
  });

  test('delivers frames that precede the marker', async () => {
    await runStream([`${SKELETON_LINE}\n${MARKER_LINE}\n`]);

    expect(frames).toHaveLength(1);
  });

  test('stops reading and ignores garbage after the marker', async () => {
    await runStream([`${MARKER_LINE}\n`, 'not json\n', `${SKELETON_LINE}\n`]);

    expect(probe.cancelled).toBe(true);
    expect(frames).toEqual([]);
    expect(errors).toEqual([]);
    expect(shutdowns).toBe(1);
  });

  test('recognises a marker split across chunks', async () => {
    await runStream(['{"event":', '"shutdown"}\n']);

    expect(shutdowns).toBe(1);
    expect(errors).toEqual([]);
  });

  test('recognises an unterminated marker in the final buffer', async () => {
    await runStream([`${SKELETON_LINE}\n${MARKER_LINE}`]);

    expect(shutdowns).toBe(1);
    expect(errors).toEqual([]);
  });

  test('does not treat a frame that mentions event as a marker', async () => {
    const lookalike = JSON.stringify({ event: 'shutdown', extra: 1 });

    await runStream([`${lookalike}\n`]);

    expect(shutdowns).toBe(0);
  });

  test('emits no shutdown for a stream that ends without the marker', async () => {
    await runStream([`${SKELETON_LINE}\n`]);

    expect(shutdowns).toBe(0);
    expect(frames).toHaveLength(1);
  });
});
