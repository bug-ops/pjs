/**
 * @jest-environment node
 *
 * Acceptance tests for #735 (track H1): HttpTransport speaks the routes a
 * stock `pjs-core` server ships: `POST /pjs/sessions` with a string
 * `client_info`, `POST /pjs/sessions/{id}/streams` with the document, then the
 * NDJSON `frames/stream` route. A superseded stream is rejected with
 * STREAM_ERROR, as the WebSocket transport does, and the Complete frame
 * carries `total_frames` = skeleton + patches + complete (#766 over HTTP).
 */

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals';
import { PJSClient } from '../../src/core/client.js';
import { HttpTransport } from '../../src/transport/http.js';
import {
  type Frame,
  PJSError,
  PJSErrorType,
  PJSEvent,
  type ProgressInfo,
  TransportType
} from '../../src/types/index.js';
import {
  COMPLETE_FRAME,
  DOCUMENT,
  FakePjsServer,
  PATCH_FRAME,
  SESSION_ID,
  SKELETON_FRAME,
  makeConfig
} from '../helpers/fake-pjs-server.js';

const flush = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));
const FRAMES = [SKELETON_FRAME, PATCH_FRAME, COMPLETE_FRAME];

let server: FakePjsServer;

afterEach(() => server.restore());

function startServer(frames: readonly unknown[] = FRAMES, stall = false): void {
  server = new FakePjsServer(frames, stall);
  server.install();
}

function makeClient(): PJSClient {
  return new PJSClient({
    baseUrl: 'http://localhost:3000',
    transport: TransportType.HTTP,
    timeout: 2000
  });
}

describe('HttpTransport session creation', () => {
  beforeEach(() => startServer());

  test('connect sends client_info as a string and returns the server session id', async () => {
    const transport = new HttpTransport(makeConfig(TransportType.HTTP));

    const connected = await transport.connect();

    const [request] = server.requestsTo('POST', '/pjs/sessions');
    expect(typeof (request.body as { client_info: unknown }).client_info).toBe('string');
    expect(connected.sessionId).toBe(SESSION_ID);
  });
});

describe('HttpTransport streaming over the shipped routes', () => {
  beforeEach(() => startServer());

  test('stream creates the stream with the document, then reads frames/stream as NDJSON', async () => {
    const client = makeClient();
    await client.connect();

    const result = await client.stream('/ignored', { data: DOCUMENT });

    expect(result).toEqual(DOCUMENT);
    const [created] = server.requestsTo('POST', `/pjs/sessions/${SESSION_ID}/streams`);
    expect(created.body).toEqual({ data: DOCUMENT });
    const [read] = server.requestsTo('GET', '/frames/stream');
    expect(read.path).toBe(new URL(server.streamUrl()).pathname);
    expect(read.accept).toBe('application/x-ndjson');
    const priority = new URLSearchParams(read.search).get('priority');
    expect(priority === null || Number(priority) <= 10).toBe(true);
    expect(server.requests.filter((r) => r.path.startsWith('/pjs/stream/'))).toEqual([]);
  });

  test('startStream without a document rejects with a PJSError before any stream request', async () => {
    const transport = new HttpTransport(makeConfig(TransportType.HTTP));
    await transport.connect();

    const outcome = await transport
      .startStream('/ignored', { sessionId: SESSION_ID, streamId: 'client-side-id' })
      .then(
        () => undefined,
        (error: unknown) => error
      );

    expect(outcome).toBeInstanceOf(PJSError);
    expect(server.requestsTo('POST', `/pjs/sessions/${SESSION_ID}/streams`)).toEqual([]);
  });

  test('the transport emits the frames of the page in order', async () => {
    const transport = new HttpTransport(makeConfig(TransportType.HTTP));
    const frames: Frame[] = [];
    transport.on('frame', (frame: Frame) => frames.push(frame));
    await transport.connect();

    await transport.startStream('/ignored', {
      sessionId: SESSION_ID,
      streamId: 'client-side-id',
      data: DOCUMENT
    });

    expect(frames.map((frame) => frame.frame_type)).toEqual(['Skeleton', 'Patch', 'Complete']);
  });

  test('the Complete frame carries total_frames = skeleton + patches + complete', async () => {
    const transport = new HttpTransport(makeConfig(TransportType.HTTP));
    const frames: Frame[] = [];
    transport.on('frame', (frame: Frame) => frames.push(frame));
    await transport.connect();

    await transport.startStream('/ignored', {
      sessionId: SESSION_ID,
      streamId: 'client-side-id',
      data: DOCUMENT
    });

    const complete = frames[frames.length - 1];
    expect(complete.frame_type).toBe('Complete');
    expect((complete as { total_frames?: number }).total_frames).toBe(3);
  });

  test('framesReceived equals totalFrames when the stream completes', async () => {
    const client = makeClient();
    const progress: ProgressInfo[] = [];
    client.on(PJSEvent.StreamComplete, ({ stats }: { stats: ProgressInfo }) => {
      progress.push({ ...stats });
    });
    await client.connect();

    await client.stream('/ignored', { data: DOCUMENT });

    expect(progress).toHaveLength(1);
    expect(progress[0].framesReceived).toBe(3);
    expect(progress[0].totalFrames).toBe(progress[0].framesReceived);
  });
});

describe('HttpTransport superseded stream', () => {
  beforeEach(() => startServer([SKELETON_FRAME], true));

  test('a second startStream rejects the first with STREAM_ERROR', async () => {
    const transport = new HttpTransport(makeConfig(TransportType.HTTP));
    await transport.connect();
    const first = transport
      .startStream('/a', { sessionId: SESSION_ID, streamId: 'a', data: { a: 1 } })
      .then(
        () => undefined,
        (error: unknown) => error
      );
    await flush();
    await flush();

    const second = transport
      .startStream('/b', { sessionId: SESSION_ID, streamId: 'b', data: { b: 2 } })
      .then(
        () => undefined,
        (error: unknown) => error
      );
    await flush();

    const error = await first;
    expect(error).toBeInstanceOf(PJSError);
    expect((error as PJSError).type).toBe(PJSErrorType.StreamError);
    await transport.disconnect();
    await second;
  });
});
