/**
 * Acceptance tests for #644 at the PJSClient level: a transport 'shutdown'
 * becomes PJSEvent.ServerShutdown followed by Disconnected{reason}, and the
 * client is no longer connected.
 */

import { describe, test, expect, beforeEach, afterEach } from '@jest/globals';
import { PJSClient } from '../../src/core/client.js';
import { PJSEvent, TransportType } from '../../src/types/index.js';

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

describe('PJSClient server shutdown', () => {
  const originalEventSource = globalThis.EventSource;
  let client: PJSClient;
  let order: string[];
  let reasons: Array<string | undefined>;

  beforeEach(async () => {
    RecordingEventSource.instances = [];
    (globalThis as { EventSource: unknown }).EventSource = RecordingEventSource;
    client = new PJSClient({
      baseUrl: 'http://localhost:3000',
      transport: TransportType.ServerSentEvents,
      timeout: 100
    });
    order = [];
    reasons = [];
    client.on(PJSEvent.ServerShutdown, () => { order.push('shutdown'); });
    client.on(PJSEvent.Disconnected, ({ reason }) => {
      order.push('disconnected');
      reasons.push(reason);
    });

    const streaming = client.stream('/s1').catch(() => undefined);
    await new Promise((resolve) => setTimeout(resolve, 0));
    RecordingEventSource.instances[0]!.dispatchEvent(
      new MessageEvent('shutdown', { data: '{"event":"shutdown"}' })
    );
    await streaming;
  });

  afterEach(() => {
    (globalThis as { EventSource: unknown }).EventSource = originalEventSource;
  });

  test('emits ServerShutdown then Disconnected', () => {
    expect(order).toEqual(['shutdown', 'disconnected']);
  });

  test('reports the server shutdown as the disconnect reason', () => {
    expect(reasons).toEqual(['server shutdown']);
  });

  test('marks the client as disconnected', () => {
    expect(client.isClientConnected()).toBe(false);
  });
});
