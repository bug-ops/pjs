/**
 * Utility Functions Tests
 * 
 * Tests for PJS client utility functions and helpers.
 */

import { describe, test, expect } from '@jest/globals';
import {
  validateFrame,
  parseJsonPath,
  calculateHeuristicPriority,
  estimateObjectSize,
  formatBytes,
  formatDuration,
  debounce,
  throttle
} from '../../src/utils/index.js';
import { FrameType, Priority } from '../../src/types/index.js';

describe('Utility Functions', () => {
  describe('validateFrame', () => {
    test('should validate skeleton frame', () => {
      const frame = {
        type: FrameType.Skeleton,
        priority: Priority.Critical,
        data: { test: true },
        timestamp: Date.now()
      };

      expect(validateFrame(frame)).toBe(true);
    });

    test('should validate patch frame', () => {
      const frame = {
        type: FrameType.Patch,
        priority: Priority.High,
        patches: [
          { path: '$.test', value: 'value', operation: 'set' }
        ],
        timestamp: Date.now()
      };

      expect(validateFrame(frame)).toBe(true);
    });

    test('should validate complete frame', () => {
      const frame = {
        type: FrameType.Complete,
        priority: Priority.Background,
        timestamp: Date.now()
      };

      expect(validateFrame(frame)).toBe(true);
    });

    test('should reject invalid frame type', () => {
      const frame = {
        type: 'invalid',
        priority: Priority.Medium
      };

      expect(validateFrame(frame)).toBe(false);
    });

    test('should reject frame without priority', () => {
      const frame = {
        type: FrameType.Skeleton,
        data: {}
      };

      expect(validateFrame(frame)).toBe(false);
    });

    test('should reject skeleton frame without data', () => {
      const frame = {
        type: FrameType.Skeleton,
        priority: Priority.Critical
      };

      expect(validateFrame(frame)).toBe(false);
    });

    test('should reject patch frame without patches', () => {
      const frame = {
        type: FrameType.Patch,
        priority: Priority.High
      };

      expect(validateFrame(frame)).toBe(false);
    });
  });

  describe('parseJsonPath', () => {
    test('should parse simple path', () => {
      const result = parseJsonPath('$.user.name');
      
      expect(result.isValid).toBe(true);
      expect(result.segments).toEqual(['user', 'name']);
      expect(result.errors).toHaveLength(0);
    });

    test('should parse array path', () => {
      const result = parseJsonPath('$.users[0].profile');
      
      expect(result.isValid).toBe(true);
      expect(result.segments).toEqual(['users[0]', 'profile']);
      expect(result.errors).toHaveLength(0);
    });

    test('should reject path not starting with $', () => {
      const result = parseJsonPath('user.name');
      
      expect(result.isValid).toBe(false);
      expect(result.errors).toContain('Path must start with "$.\"');
    });

    test('should detect empty segments', () => {
      const result = parseJsonPath('$.user..name');
      
      expect(result.isValid).toBe(false);
      expect(result.errors.some(e => e.includes('Empty segment'))).toBe(true);
    });

    test('should validate array indices', () => {
      const result = parseJsonPath('$.users[-1].name');
      
      expect(result.isValid).toBe(false);
      expect(result.errors.some(e => e.includes('Invalid array index'))).toBe(true);
    });
  });

  describe('calculateHeuristicPriority', () => {
    test('should assign critical priority to user ID', () => {
      const priority = calculateHeuristicPriority('$.user.id', 123);
      expect(priority).toBe(Priority.Critical);
    });

    test('should assign critical priority to status', () => {
      const priority = calculateHeuristicPriority('$.status', 'active');
      expect(priority).toBe(Priority.Critical);
    });

    test('should assign high priority to names', () => {
      const priority = calculateHeuristicPriority('$.user.name', 'John Doe');
      expect(priority).toBe(Priority.High);
    });

    test('should assign high priority to prices', () => {
      const priority = calculateHeuristicPriority('$.product.price', 29.99);
      expect(priority).toBe(Priority.High);
    });

    test('should assign medium priority to descriptions', () => {
      const priority = calculateHeuristicPriority('$.product.description', 'A great product');
      expect(priority).toBe(Priority.Medium);
    });

    test('should assign low priority to timestamps', () => {
      const priority = calculateHeuristicPriority('$.created_at', '2024-01-01');
      expect(priority).toBe(Priority.Low);
    });

    test('should assign background priority to large arrays', () => {
      const largeArray = new Array(15).fill('item');
      const priority = calculateHeuristicPriority('$.items', largeArray);
      expect(priority).toBe(Priority.Background);
    });

    test('should assign background priority to analytics', () => {
      const priority = calculateHeuristicPriority('$.analytics.stats', {});
      expect(priority).toBe(Priority.Background);
    });

    test('should assign low priority to long strings', () => {
      const longString = 'x'.repeat(2000);
      const priority = calculateHeuristicPriority('$.content', longString);
      expect(priority).toBe(Priority.Low);
    });
  });

  describe('estimateObjectSize', () => {
    test('should estimate size of primitive values', () => {
      expect(estimateObjectSize('hello')).toBe(10); // 5 chars * 2 bytes
      expect(estimateObjectSize(42)).toBe(8);
      expect(estimateObjectSize(true)).toBe(4);
      expect(estimateObjectSize(null)).toBe(0);
    });

    test('should estimate size of simple object', () => {
      const obj = { name: 'John', age: 30 };
      const size = estimateObjectSize(obj);
      
      // Should account for keys and values
      expect(size).toBeGreaterThan(0);
      expect(size).toBeLessThan(100);
    });

    test('should estimate size of array', () => {
      const arr = ['a', 'b', 'c'];
      const size = estimateObjectSize(arr);
      
      expect(size).toBeGreaterThan(0);
      expect(size).toBeLessThan(50);
    });

    test('should handle circular references', () => {
      const obj: any = { name: 'test' };
      obj.self = obj;
      
      // Should not throw or loop infinitely
      expect(() => estimateObjectSize(obj)).not.toThrow();
      expect(estimateObjectSize(obj)).toBeGreaterThan(0);
    });
  });

  describe('formatBytes', () => {
    test('should format bytes correctly', () => {
      expect(formatBytes(0)).toBe('0.0 B');
      expect(formatBytes(512)).toBe('512.0 B');
      expect(formatBytes(1024)).toBe('1.0 KB');
      expect(formatBytes(1536)).toBe('1.5 KB');
      expect(formatBytes(1048576)).toBe('1.0 MB');
      expect(formatBytes(1073741824)).toBe('1.0 GB');
    });
  });

  describe('formatDuration', () => {
    test('should format durations correctly', () => {
      expect(formatDuration(100)).toBe('100ms');
      expect(formatDuration(1500)).toBe('1.5s');
      expect(formatDuration(65000)).toBe('1m 5s');
      expect(formatDuration(3665000)).toBe('1h 1m');
    });
  });

  describe('debounce', () => {
    test('should debounce function calls', async () => {
      let callCount = 0;
      const increment = () => callCount++;
      const debouncedIncrement = debounce(increment, 100);

      // Call multiple times rapidly
      debouncedIncrement();
      debouncedIncrement();
      debouncedIncrement();

      // Should not have called yet
      expect(callCount).toBe(0);

      // Wait for debounce period
      await new Promise(resolve => setTimeout(resolve, 150));
      
      // Should have called once
      expect(callCount).toBe(1);
    });

    test('should allow canceling debounced function', () => {
      let callCount = 0;
      const increment = () => callCount++;
      const debouncedIncrement = debounce(increment, 100);

      debouncedIncrement();
      debouncedIncrement.cancel();

      // Wait longer than debounce period
      setTimeout(() => {
        expect(callCount).toBe(0);
      }, 150);
    });
  });

  describe('throttle', () => {
    test('should throttle function calls', async () => {
      let callCount = 0;
      const increment = () => callCount++;
      const throttledIncrement = throttle(increment, 100);

      // Call multiple times rapidly
      throttledIncrement();
      throttledIncrement();
      throttledIncrement();

      // Should have called immediately once
      expect(callCount).toBe(0);

      // Wait for throttle period
      await new Promise(resolve => setTimeout(resolve, 150));
      
      // Should have called the last invocation
      expect(callCount).toBe(1);
    });

    test('should allow canceling throttled function', () => {
      let callCount = 0;
      const increment = () => callCount++;
      const throttledIncrement = throttle(increment, 100);

      throttledIncrement();
      throttledIncrement.cancel();

      setTimeout(() => {
        expect(callCount).toBe(0);
      }, 150);
    });
  });
});