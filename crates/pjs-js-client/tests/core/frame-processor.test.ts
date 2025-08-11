/**
 * Frame Processor Tests
 * 
 * Tests for PJS frame validation and processing logic.
 */

import { describe, test, expect, beforeEach } from '@jest/globals';
import { FrameProcessor } from '../../src/core/frame-processor.js';
import { FrameType, Priority, PJSErrorType } from '../../src/types/index.js';

describe('FrameProcessor', () => {
  let processor: FrameProcessor;

  beforeEach(() => {
    processor = new FrameProcessor();
  });

  describe('Frame Validation', () => {
    test('should validate well-formed skeleton frame', () => {
      const frame = {
        type: FrameType.Skeleton,
        priority: Priority.Critical,
        data: { user: { name: null, id: null } },
        complete: false,
        timestamp: Date.now()
      };

      const result = processor.validateFrame(frame);
      
      expect(result.isValid).toBe(true);
      expect(result.errors).toHaveLength(0);
    });

    test('should validate well-formed patch frame', () => {
      const frame = {
        type: FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: '$.user.name',
            value: 'John Doe',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const result = processor.validateFrame(frame);
      
      expect(result.isValid).toBe(true);
      expect(result.errors).toHaveLength(0);
    });

    test('should validate well-formed complete frame', () => {
      const frame = {
        type: FrameType.Complete,
        priority: Priority.Background,
        timestamp: Date.now(),
        checksum: 'sha256:abc123'
      };

      const result = processor.validateFrame(frame);
      
      expect(result.isValid).toBe(true);
      expect(result.errors).toHaveLength(0);
    });

    test('should reject frame with missing type', () => {
      const frame = {
        priority: Priority.High,
        data: {}
      };

      const result = processor.validateFrame(frame as any);
      
      expect(result.isValid).toBe(false);
      expect(result.errors.some(e => e.includes('type'))).toBe(true);
    });

    test('should reject frame with invalid type', () => {
      const frame = {
        type: 'invalid_type',
        priority: Priority.High,
        data: {}
      };

      const result = processor.validateFrame(frame as any);
      
      expect(result.isValid).toBe(false);
      expect(result.errors.some(e => e.includes('Invalid frame type'))).toBe(true);
    });

    test('should reject frame with missing priority', () => {
      const frame = {
        type: FrameType.Skeleton,
        data: {}
      };

      const result = processor.validateFrame(frame as any);
      
      expect(result.isValid).toBe(false);
      expect(result.errors.some(e => e.includes('priority'))).toBe(true);
    });

    test('should reject frame with invalid priority range', () => {
      const frame = {
        type: FrameType.Skeleton,
        priority: 150, // Out of range
        data: {}
      };

      const result = processor.validateFrame(frame as any);
      
      expect(result.isValid).toBe(false);
      expect(result.errors.some(e => e.includes('Priority must be'))).toBe(true);
    });

    test('should reject skeleton frame without data', () => {
      const frame = {
        type: FrameType.Skeleton,
        priority: Priority.Critical
      };

      const result = processor.validateFrame(frame as any);
      
      expect(result.isValid).toBe(false);
      expect(result.errors.some(e => e.includes('data field'))).toBe(true);
    });

    test('should reject patch frame without patches', () => {
      const frame = {
        type: FrameType.Patch,
        priority: Priority.High
      };

      const result = processor.validateFrame(frame as any);
      
      expect(result.isValid).toBe(false);
      expect(result.errors.some(e => e.includes('patches array'))).toBe(true);
    });

    test('should reject patch frame with empty patches array', () => {
      const frame = {
        type: FrameType.Patch,
        priority: Priority.High,
        patches: []
      };

      const result = processor.validateFrame(frame as any);
      
      expect(result.isValid).toBe(false);
      expect(result.errors.some(e => e.includes('at least one patch'))).toBe(true);
    });

    test('should validate patch operations', () => {
      const validFrame = {
        type: FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: '$.test',
            value: 'value',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const invalidFrame = {
        type: FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: '$.test',
            value: 'value',
            operation: 'invalid_op' as any
          }
        ],
        timestamp: Date.now()
      };

      expect(processor.validateFrame(validFrame).isValid).toBe(true);
      expect(processor.validateFrame(invalidFrame).isValid).toBe(false);
    });

    test('should validate JSON paths in patches', () => {
      const validFrame = {
        type: FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: '$.valid.path',
            value: 'value',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const invalidFrame = {
        type: FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: 'invalid.path', // Missing $.
            value: 'value',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      expect(processor.validateFrame(validFrame).isValid).toBe(true);
      expect(processor.validateFrame(invalidFrame).isValid).toBe(false);
    });
  });

  describe('Frame Processing State', () => {
    test('should track expected frame sequence', () => {
      expect(processor.getExpectedFrameType()).toBe(FrameType.Skeleton);

      // Process skeleton
      const skeleton = {
        type: FrameType.Skeleton,
        priority: Priority.Critical,
        data: { test: null },
        complete: false,
        timestamp: Date.now()
      };

      const skeletonResult = processor.processFrame(skeleton);
      expect(skeletonResult.accepted).toBe(true);
      expect(processor.getExpectedFrameType()).toBe(FrameType.Patch);

      // Process patch
      const patch = {
        type: FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: '$.test',
            value: 'value',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const patchResult = processor.processFrame(patch);
      expect(patchResult.accepted).toBe(true);
      expect(processor.getExpectedFrameType()).toBe(FrameType.Patch);

      // Process complete
      const complete = {
        type: FrameType.Complete,
        priority: Priority.Background,
        timestamp: Date.now()
      };

      const completeResult = processor.processFrame(complete);
      expect(completeResult.accepted).toBe(true);
      expect(processor.isStreamComplete()).toBe(true);
    });

    test('should reject out-of-order frames', () => {
      // Try to process patch before skeleton
      const patch = {
        type: FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: '$.test',
            value: 'value',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const result = processor.processFrame(patch);
      
      expect(result.accepted).toBe(false);
      expect(result.error?.type).toBe(PJSErrorType.ProtocolViolation);
      expect(result.error?.message).toContain('Expected skeleton');
    });

    test('should reject duplicate skeleton frames', () => {
      const skeleton = {
        type: FrameType.Skeleton,
        priority: Priority.Critical,
        data: { test: null },
        complete: false,
        timestamp: Date.now()
      };

      // First skeleton should be accepted
      const firstResult = processor.processFrame(skeleton);
      expect(firstResult.accepted).toBe(true);

      // Second skeleton should be rejected
      const secondResult = processor.processFrame(skeleton);
      expect(secondResult.accepted).toBe(false);
      expect(secondResult.error?.type).toBe(PJSErrorType.ProtocolViolation);
    });

    test('should reject frames after completion', () => {
      // Process complete sequence
      const skeleton = {
        type: FrameType.Skeleton,
        priority: Priority.Critical,
        data: { test: null },
        complete: false,
        timestamp: Date.now()
      };

      const complete = {
        type: FrameType.Complete,
        priority: Priority.Background,
        timestamp: Date.now()
      };

      processor.processFrame(skeleton);
      processor.processFrame(complete);

      // Try to process another frame
      const patch = {
        type: FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: '$.test',
            value: 'value',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const result = processor.processFrame(patch);
      
      expect(result.accepted).toBe(false);
      expect(result.error?.type).toBe(PJSErrorType.ProtocolViolation);
      expect(result.error?.message).toContain('Stream is already complete');
    });
  });

  describe('Priority Validation', () => {
    test('should enforce non-increasing priority order', () => {
      // Initialize processor
      const skeleton = {
        type: FrameType.Skeleton,
        priority: Priority.Critical,
        data: { test: null },
        complete: false,
        timestamp: Date.now()
      };

      processor.processFrame(skeleton);

      // First patch with high priority
      const highPriorityPatch = {
        type: FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: '$.test1',
            value: 'value1',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const result1 = processor.processFrame(highPriorityPatch);
      expect(result1.accepted).toBe(true);

      // Second patch with even higher priority (should be rejected)
      const higherPriorityPatch = {
        type: FrameType.Patch,
        priority: Priority.Critical,
        patches: [
          {
            path: '$.test2',
            value: 'value2',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const result2 = processor.processFrame(higherPriorityPatch);
      
      expect(result2.accepted).toBe(false);
      expect(result2.error?.type).toBe(PJSErrorType.ProtocolViolation);
      expect(result2.error?.message).toContain('Priority order violation');
    });

    test('should allow same priority patches', () => {
      const skeleton = {
        type: FrameType.Skeleton,
        priority: Priority.Critical,
        data: { test: null },
        complete: false,
        timestamp: Date.now()
      };

      processor.processFrame(skeleton);

      const patch1 = {
        type: FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: '$.test1',
            value: 'value1',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const patch2 = {
        type: FrameType.Patch,
        priority: Priority.High, // Same priority
        patches: [
          {
            path: '$.test2',
            value: 'value2',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const result1 = processor.processFrame(patch1);
      const result2 = processor.processFrame(patch2);
      
      expect(result1.accepted).toBe(true);
      expect(result2.accepted).toBe(true);
    });
  });

  describe('Statistics and Metadata', () => {
    test('should track processing statistics', () => {
      // Process complete stream
      const skeleton = {
        type: FrameType.Skeleton,
        priority: Priority.Critical,
        data: { test: null },
        complete: false,
        timestamp: Date.now()
      };

      const patch1 = {
        type: FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: '$.test',
            value: 'value',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const patch2 = {
        type: FrameType.Patch,
        priority: Priority.Low,
        patches: [
          {
            path: '$.other',
            value: 'other',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const complete = {
        type: FrameType.Complete,
        priority: Priority.Background,
        timestamp: Date.now()
      };

      processor.processFrame(skeleton);
      processor.processFrame(patch1);
      processor.processFrame(patch2);
      processor.processFrame(complete);

      const stats = processor.getStatistics();
      
      expect(stats.framesProcessed).toBe(4);
      expect(stats.patchesApplied).toBe(2);
      expect(stats.priorityDistribution[Priority.Critical]).toBe(1);
      expect(stats.priorityDistribution[Priority.High]).toBe(1);
      expect(stats.priorityDistribution[Priority.Low]).toBe(1);
      expect(stats.priorityDistribution[Priority.Background]).toBe(1);
    });

    test('should reset state correctly', () => {
      // Process some frames
      const skeleton = {
        type: FrameType.Skeleton,
        priority: Priority.Critical,
        data: { test: null },
        complete: false,
        timestamp: Date.now()
      };

      processor.processFrame(skeleton);

      expect(processor.isStreamComplete()).toBe(false);
      expect(processor.getStatistics().framesProcessed).toBe(1);

      // Reset
      processor.reset();

      expect(processor.isStreamComplete()).toBe(false);
      expect(processor.getExpectedFrameType()).toBe(FrameType.Skeleton);
      expect(processor.getStatistics().framesProcessed).toBe(0);
    });
  });
});