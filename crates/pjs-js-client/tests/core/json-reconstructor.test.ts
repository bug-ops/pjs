/**
 * JSON Reconstructor Tests
 * 
 * Tests for progressive JSON reconstruction from skeleton and patches.
 */

import { describe, test, expect, beforeEach } from '@jest/globals';
import { JsonReconstructor } from '../../src/core/json-reconstructor.js';
import { FrameType, Priority } from '../../src/types/index.js';

describe('JsonReconstructor', () => {
  let reconstructor: JsonReconstructor;

  beforeEach(() => {
    reconstructor = new JsonReconstructor();
  });

  describe('Skeleton Processing', () => {
    test('should process skeleton frame', () => {
      const skeletonFrame = {
        type: FrameType.Skeleton as FrameType.Skeleton,
        priority: Priority.Critical,
        data: {
          user: {
            id: null,
            name: null,
            profile: {
              bio: null,
              avatar: null
            }
          },
          posts: []
        },
        complete: false,
        timestamp: Date.now()
      };

      const result = reconstructor.processSkeleton(skeletonFrame);

      expect(result.success).toBe(true);
      expect(result.data).toEqual(skeletonFrame.data);
      expect(reconstructor.getCurrentState()).toEqual(skeletonFrame.data);
    });

    test('should reject invalid skeleton frame', () => {
      const invalidFrame = {
        type: FrameType.Skeleton as FrameType.Skeleton,
        priority: Priority.Critical,
        // Missing data field
        complete: false,
        timestamp: Date.now()
      } as any;

      const result = reconstructor.processSkeleton(invalidFrame);

      expect(result.success).toBe(false);
      expect(result.error).toBeDefined();
    });

    test('should reject skeleton when already initialized', () => {
      const skeletonFrame = {
        type: FrameType.Skeleton as FrameType.Skeleton,
        priority: Priority.Critical,
        data: { test: true },
        complete: false,
        timestamp: Date.now()
      };

      // First skeleton should succeed
      reconstructor.processSkeleton(skeletonFrame);

      // Second skeleton should fail
      const result = reconstructor.processSkeleton(skeletonFrame);
      
      expect(result.success).toBe(false);
      expect(result.error).toContain('already initialized');
    });
  });

  describe('Patch Processing', () => {
    beforeEach(() => {
      // Initialize with skeleton
      const skeleton = {
        type: FrameType.Skeleton as FrameType.Skeleton,
        priority: Priority.Critical,
        data: {
          user: {
            id: null,
            name: null,
            email: null
          },
          posts: [],
          metadata: {
            created: null,
            updated: null
          }
        },
        complete: false,
        timestamp: Date.now()
      };

      reconstructor.processSkeleton(skeleton);
    });

    test('should apply simple patch', () => {
      const patchFrame = {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: '$.user.id',
            value: 123,
            operation: 'set' as const
          },
          {
            path: '$.user.name', 
            value: 'John Doe',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const result = reconstructor.applyPatch(patchFrame);

      expect(result.success).toBe(true);
      expect(result.appliedPatches).toBe(2);

      const state = reconstructor.getCurrentState();
      expect(state.user.id).toBe(123);
      expect(state.user.name).toBe('John Doe');
    });

    test('should handle append operation for arrays', () => {
      const patchFrame = {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.Medium,
        patches: [
          {
            path: '$.posts',
            value: [
              { id: 1, title: 'First Post' },
              { id: 2, title: 'Second Post' }
            ],
            operation: 'append' as const
          }
        ],
        timestamp: Date.now()
      };

      const result = reconstructor.applyPatch(patchFrame);

      expect(result.success).toBe(true);
      expect(result.appliedPatches).toBe(1);

      const state = reconstructor.getCurrentState();
      expect(state.posts).toHaveLength(2);
      expect(state.posts[0].title).toBe('First Post');
    });

    test('should handle merge operation for objects', () => {
      // First set some initial data
      const initialPatch = {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: '$.metadata',
            value: { created: '2024-01-01', version: 1 },
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      reconstructor.applyPatch(initialPatch);

      // Then merge additional data
      const mergePatch = {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.Low,
        patches: [
          {
            path: '$.metadata',
            value: { updated: '2024-01-15', author: 'user123' },
            operation: 'merge' as const
          }
        ],
        timestamp: Date.now()
      };

      const result = reconstructor.applyPatch(mergePatch);

      expect(result.success).toBe(true);

      const state = reconstructor.getCurrentState();
      expect(state.metadata.created).toBe('2024-01-01'); // Preserved
      expect(state.metadata.updated).toBe('2024-01-15'); // Added
      expect(state.metadata.version).toBe(1); // Preserved
      expect(state.metadata.author).toBe('user123'); // Added
    });

    test('should handle nested path application', () => {
      const patchFrame = {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.Medium,
        patches: [
          {
            path: '$.user.profile.settings.theme',
            value: 'dark',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      // This should create intermediate objects
      const result = reconstructor.applyPatch(patchFrame);

      expect(result.success).toBe(true);

      const state = reconstructor.getCurrentState();
      expect(state.user.profile.settings.theme).toBe('dark');
    });

    test('should reject patch without skeleton', () => {
      const freshReconstructor = new JsonReconstructor();
      
      const patchFrame = {
        type: FrameType.Patch as FrameType.Patch,
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

      const result = freshReconstructor.applyPatch(patchFrame);

      expect(result.success).toBe(false);
      expect(result.error).toContain('not initialized');
    });

    test('should handle invalid JSON path', () => {
      const patchFrame = {
        type: FrameType.Patch as FrameType.Patch,
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

      const result = reconstructor.applyPatch(patchFrame);

      expect(result.success).toBe(false);
      expect(result.skippedPatches).toBe(1);
    });

    test('should track patch application statistics', () => {
      const patchFrame = {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.High,
        patches: [
          {
            path: '$.user.name',
            value: 'John',
            operation: 'set' as const
          },
          {
            path: 'invalid',
            value: 'bad',
            operation: 'set' as const
          }
        ],
        timestamp: Date.now()
      };

      const result = reconstructor.applyPatch(patchFrame);

      expect(result.appliedPatches).toBe(1);
      expect(result.skippedPatches).toBe(1);
      expect(result.totalPatches).toBe(2);
    });
  });

  describe('State Management', () => {
    test('should track completion status', () => {
      expect(reconstructor.isComplete()).toBe(false);

      // Add skeleton
      const skeleton = {
        type: FrameType.Skeleton as FrameType.Skeleton,
        priority: Priority.Critical,
        data: { test: null },
        complete: false,
        timestamp: Date.now()
      };

      reconstructor.processSkeleton(skeleton);
      expect(reconstructor.isComplete()).toBe(false);

      // Mark as complete
      reconstructor.markComplete();
      expect(reconstructor.isComplete()).toBe(true);
    });

    test('should provide metadata', () => {
      const skeleton = {
        type: FrameType.Skeleton as FrameType.Skeleton,
        priority: Priority.Critical,
        data: { test: null },
        complete: false,
        timestamp: Date.now()
      };

      reconstructor.processSkeleton(skeleton);

      const metadata = reconstructor.getMetadata();
      
      expect(metadata.isInitialized).toBe(true);
      expect(metadata.isComplete).toBe(false);
      expect(metadata.patchesApplied).toBe(0);
      expect(metadata.memoryUsage).toBeGreaterThan(0);
      expect(typeof metadata.estimatedSize).toBe('number');
    });

    test('should reset state correctly', () => {
      // Initialize with data
      const skeleton = {
        type: FrameType.Skeleton as FrameType.Skeleton,
        priority: Priority.Critical,
        data: { test: 'value' },
        complete: false,
        timestamp: Date.now()
      };

      reconstructor.processSkeleton(skeleton);
      reconstructor.markComplete();

      expect(reconstructor.isComplete()).toBe(true);
      expect(reconstructor.getCurrentState()).toEqual({ test: 'value' });

      // Reset
      reconstructor.reset();

      expect(reconstructor.isComplete()).toBe(false);
      expect(reconstructor.getMetadata().isInitialized).toBe(false);
      expect(reconstructor.getCurrentState()).toBeNull();
    });
  });

  describe('Memory Management', () => {
    test('should track memory usage', () => {
      const skeleton = {
        type: FrameType.Skeleton as FrameType.Skeleton,
        priority: Priority.Critical,
        data: {
          largeArray: new Array(1000).fill('data'),
          nestedObject: {
            level1: { level2: { level3: 'deep' } }
          }
        },
        complete: false,
        timestamp: Date.now()
      };

      reconstructor.processSkeleton(skeleton);

      const metadata = reconstructor.getMetadata();
      
      expect(metadata.memoryUsage).toBeGreaterThan(1000);
      expect(metadata.estimatedSize).toBeGreaterThan(1000);
    });

    test('should handle large object reconstruction', () => {
      const skeleton = {
        type: FrameType.Skeleton as FrameType.Skeleton,
        priority: Priority.Critical,
        data: {
          items: []
        },
        complete: false,
        timestamp: Date.now()
      };

      reconstructor.processSkeleton(skeleton);

      // Add many items
      const items = Array.from({ length: 100 }, (_, i) => ({
        id: i,
        name: `Item ${i}`,
        description: `Description for item ${i}`.repeat(10)
      }));

      const patchFrame = {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.Low,
        patches: [
          {
            path: '$.items',
            value: items,
            operation: 'append' as const
          }
        ],
        timestamp: Date.now()
      };

      const result = reconstructor.applyPatch(patchFrame);

      expect(result.success).toBe(true);
      expect(reconstructor.getCurrentState().items).toHaveLength(100);
      
      const metadata = reconstructor.getMetadata();
      expect(metadata.memoryUsage).toBeGreaterThan(10000);
    });
  });
});