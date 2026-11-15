/**
 * A pending stream() must reject with PJSErrorType.ServerShutdown when the
 * transport reports a server shutdown before StreamComplete (#644).
 */

import { EventEmitter } from 'events';
import { describe, test, expect, jest } from '@jest/globals';
import { PJSClient } from '../../src/core/client.js';
import { SHUTDOWN_EVENT, type Transport } from '../../src/transport/base.js';
import { PJSError, PJSErrorType } from '../../src/types/index.js';

describe('PJSClient pending stream on server shutdown', () => {
  test('rejects with PJSErrorType.ServerShutdown', async () => {
    const transport = new EventEmitter();
    const client = new PJSClient({ baseUrl: 'http://localhost:3000', timeout: 1000 });
    Object.assign(transport, {
      connect: () => Promise.resolve({ sessionId: 's1' }),
      disconnect: () => Promise.resolve(),
      startStream: jest.fn(() => {
        setTimeout(() => transport.emit(SHUTDOWN_EVENT), 0);
        return new Promise<void>(() => {});
      })
    });
    (client as unknown as { _transport: Transport })._transport = transport as unknown as Transport;
    (client as unknown as { setupEventHandlers(): void }).setupEventHandlers();

    const error = await client.stream('/s1').catch((e: unknown) => e);

    expect(error).toBeInstanceOf(PJSError);
    expect((error as PJSError).type).toBe(PJSErrorType.ServerShutdown);
  });
});
