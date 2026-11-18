/**
 * Shared scripted WebSocket double and `WsMessage` builders for the
 * WebSocketTransport acceptance tests. The transport is driven through the
 * `on*` handler properties only.
 */

import {
  Priority,
  TransportType,
  type PJSClientConfig
} from '../../src/types/index.js';

export class FakeSocket {
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

  static latest(): FakeSocket {
    return FakeSocket.instances[FakeSocket.instances.length - 1];
  }
}

export const UUID_1 = '11111111-1111-4111-8111-111111111111';
export const UUID_2 = '33333333-3333-4333-8333-333333333333';

export function makeConfig(baseUrl = 'http://localhost:3000'): Required<PJSClientConfig> {
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

export const flush = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));

export type Outcome = { readonly ok: true } | { readonly ok: false; readonly error: unknown };

export function settle(promise: Promise<unknown>): Promise<Outcome> {
  return promise.then(
    (): Outcome => ({ ok: true }),
    (error: unknown): Outcome => ({ ok: false, error })
  );
}

/** Resolves to the outcome, or `'pending'` when the promise has not settled after a macrotask. */
export async function settledOrPending(outcome: Promise<Outcome>): Promise<Outcome | 'pending'> {
  return Promise.race([outcome, flush().then((): 'pending' => 'pending')]);
}

export function patchMessage(frameId: number, patches: unknown[], sessionId: string, priority = 80): unknown {
  return {
    type: 'StreamPatch',
    data: { session_id: sessionId, frame_id: frameId, priority, patches, is_complete: false }
  };
}

export const completeMessage = (sessionId: string): unknown => ({
  type: 'StreamComplete',
  data: { session_id: sessionId }
});

export const errorMessage = (sessionId: string | null, code: number, error = 'boom'): unknown => ({
  type: 'Error',
  data: { session_id: sessionId, error, code }
});

const originalWebSocket = Object.getOwnPropertyDescriptor(globalThis, 'WebSocket');
const originalCrypto = Object.getOwnPropertyDescriptor(globalThis, 'crypto');

function restoreGlobal(name: string, descriptor: PropertyDescriptor | undefined): void {
  if (descriptor) {
    Object.defineProperty(globalThis, name, descriptor);
  } else {
    Reflect.deleteProperty(globalThis, name);
  }
}

/** Installs `FakeSocket` as `globalThis.WebSocket` and a deterministic `crypto.randomUUID`. */
export function installFakes(...uuids: string[]): void {
  FakeSocket.instances = [];
  Object.defineProperty(globalThis, 'WebSocket', { value: FakeSocket, configurable: true, writable: true });
  let next = 0;
  Object.defineProperty(globalThis, 'crypto', {
    value: { randomUUID: () => uuids[Math.min(next++, uuids.length - 1)] },
    configurable: true,
    writable: true
  });
}

export function restoreFakes(): void {
  restoreGlobal('WebSocket', originalWebSocket);
  restoreGlobal('crypto', originalCrypto);
}
