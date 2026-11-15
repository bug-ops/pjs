/**
 * Acceptance tests for the shutdown marker guard shared by the SSE and NDJSON
 * transports (#644).
 */

import { describe, test, expect } from '@jest/globals';
import { SHUTDOWN_EVENT, isShutdownMarker } from '../../src/transport/base.js';

describe('SHUTDOWN_EVENT', () => {
  test('is the wire name of the named SSE event', () => {
    expect(SHUTDOWN_EVENT).toBe('shutdown');
  });
});

describe('isShutdownMarker', () => {
  test.each([[{ event: 'shutdown' }]])('accepts %j', (value) => {
    expect(isShutdownMarker(value)).toBe(true);
  });

  test.each([
    [{ event: 'shutdown', extra: 1 }],
    [{ event: 'other' }],
    [{ event: 1 }],
    [{}],
    [{ frame_type: 'skeleton', priority: 100, payload: {} }],
    [[{ event: 'shutdown' }]],
    ['shutdown'],
    [null],
    [undefined],
    [42]
  ])('rejects %j', (value) => {
    expect(isShutdownMarker(value)).toBe(false);
  });
});
