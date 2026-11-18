/**
 * Scripted double of the shipped `pjs-core` HTTP routes for the HTTP and SSE
 * transport acceptance tests (#735). It answers only the routes a stock
 * `create_pjs_router*` server serves and records every request, so a test can
 * assert both the flow and the wire shapes. Any other path answers 404.
 */

import {
  Priority,
  TransportType,
  type PJSClientConfig
} from '../../src/types/index.js';

export const SESSION_ID = '11111111-1111-4111-8111-111111111111';
export const STREAM_ID = '22222222-2222-4222-8222-222222222222';

export interface RecordedRequest {
  method: string;
  path: string;
  search: string;
  accept: string | undefined;
  body: unknown;
}

export const SKELETON_FRAME = {
  stream_id: STREAM_ID,
  frame_type: 'Skeleton',
  priority: 100,
  sequence: 0,
  payload: { name: null, list: [] }
};

export const PATCH_FRAME = {
  stream_id: STREAM_ID,
  frame_type: 'Patch',
  priority: 75,
  sequence: 1,
  payload: {
    patches: [
      { path: '$.name', value: 'ada', operation: 'set' },
      { path: '$.list', value: [1, 2], operation: 'set' }
    ]
  }
};

export const COMPLETE_FRAME = {
  stream_id: STREAM_ID,
  frame_type: 'Complete',
  priority: 100,
  sequence: 2,
  payload: {}
};

export const DOCUMENT = { name: 'ada', list: [1, 2] };

export function makeConfig(
  transport: TransportType,
  baseUrl = 'http://localhost:3000'
): Required<PJSClientConfig> {
  return {
    baseUrl,
    transport,
    sessionId: 'unused',
    headers: {},
    timeout: 2000,
    debug: false,
    bufferSize: 1024,
    priorityThreshold: Priority.Background,
    maxConcurrentStreams: 1,
    maxStreamBytes: 16 * 1024 * 1024,
    stateDelivery: 'live'
  };
}

export class FakePjsServer {
  readonly requests: RecordedRequest[] = [];
  private readonly originalFetch = globalThis.fetch;

  /**
   * `frames` are the lines of the `frames/stream` NDJSON body, in order. With
   * `stall`, the body then stays open (until the request is aborted) instead
   * of ending, like a server that is still producing frames.
   */
  constructor(
    private readonly frames: readonly unknown[],
    private readonly stall = false
  ) {}

  install(): void {
    globalThis.fetch = (input: unknown, init?: RequestInit): Promise<Response> =>
      this.handle(String(input instanceof URL ? input.href : input), init);
  }

  restore(): void {
    globalThis.fetch = this.originalFetch;
  }

  requestsTo(method: string, pathSuffix: string): RecordedRequest[] {
    return this.requests.filter((r) => r.method === method && r.path.endsWith(pathSuffix));
  }

  streamUrl(): string {
    return `http://localhost:3000/pjs/sessions/${SESSION_ID}/streams/${STREAM_ID}/frames/stream`;
  }

  private json(value: unknown, status = 200): Promise<Response> {
    return Promise.resolve(
      new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json' } })
    );
  }

  private handle(url: string, init?: RequestInit): Promise<Response> {
    const parsed = new URL(url);
    const method = (init?.method ?? 'GET').toUpperCase();
    const headers = new Headers(init?.headers);
    const rawBody = typeof init?.body === 'string' ? init.body : undefined;
    this.requests.push({
      method,
      path: parsed.pathname,
      search: parsed.search,
      accept: headers.get('accept') ?? undefined,
      body: rawBody === undefined || rawBody === '' ? undefined : (JSON.parse(rawBody) as unknown)
    });

    const sessions = '/pjs/sessions';
    const stream = `${sessions}/${SESSION_ID}/streams`;
    if (method === 'POST' && parsed.pathname === sessions) {
      const clientInfo = (this.requests[this.requests.length - 1].body as { client_info?: unknown } | undefined)
        ?.client_info;
      if (clientInfo !== undefined && typeof clientInfo !== 'string') {
        return this.json({ error: 'client_info must be a string' }, 422);
      }
      return this.json({ session_id: SESSION_ID, expires_at: '2030-01-01T00:00:00Z' });
    }
    if (method === 'POST' && parsed.pathname === stream) {
      return this.json({ stream_id: STREAM_ID, status: 'created' });
    }
    if (method === 'POST' && parsed.pathname.startsWith(`${stream}/${STREAM_ID}/`)) {
      return this.json({ stream_id: STREAM_ID, status: 'ok', frames: [], total: 0 });
    }
    if (method === 'GET' && parsed.pathname === `${stream}/${STREAM_ID}/frames/stream`) {
      const text = this.frames.map((frame) => `${JSON.stringify(frame)}\n`).join('');
      if (!this.stall) {
        return Promise.resolve(
          new Response(text, { status: 200, headers: { 'content-type': 'application/x-ndjson' } })
        );
      }
      const signal = init?.signal;
      const body = new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new TextEncoder().encode(text));
          signal?.addEventListener('abort', () =>
            controller.error(new DOMException('aborted', 'AbortError'))
          );
        }
      });
      return Promise.resolve(
        new Response(body, { status: 200, headers: { 'content-type': 'application/x-ndjson' } })
      );
    }
    return this.json({ error: 'not found' }, 404);
  }
}
