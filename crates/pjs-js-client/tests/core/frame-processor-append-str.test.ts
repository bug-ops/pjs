/**
 * Acceptance tests for #731: the frame validator derives its operation list
 * from `PATCH_OPERATIONS`, so `append_str` is accepted and unknown operations
 * stay rejected.
 */

import { describe, test, expect } from '@jest/globals';
import { FrameProcessor } from '../../src/core/frame-processor.js';
import { PATCH_OPERATIONS } from '../../src/types/index.js';

function patchFrame(operation: unknown): unknown {
  return {
    frame_type: 'Patch',
    priority: 80,
    payload: { patches: [{ operation, path: '$.s', value: 'x' }] }
  };
}

describe('PATCH_OPERATIONS', () => {
  test('lists the five wire operations in order', () => {
    expect([...PATCH_OPERATIONS]).toEqual(['set', 'append', 'merge', 'delete', 'append_str']);
  });
});

describe('FrameProcessor.validateFrame operations', () => {
  test.each([...PATCH_OPERATIONS])('accepts the %s operation', (operation) => {
    expect(new FrameProcessor().validateFrame(patchFrame(operation))).toEqual({ isValid: true, errors: [] });
  });

  test.each(['explode', 'APPEND_STR', 'append-str', '', 7, null])('rejects the operation %j', (operation) => {
    const result = new FrameProcessor().validateFrame(patchFrame(operation));

    expect(result.isValid).toBe(false);
    expect(result.errors).toEqual([`Patch operation 0 has invalid operation: ${String(operation)}`]);
  });
});
