/**
 * WasmBackend Integration Tests
 *
 * Tests the complete WASM backend transport including:
 * - Initialization and connection
 * - Frame streaming from JSON data
 * - Priority filtering
 * - Error handling
 * - Memory cleanup
 */

import { describe, test, expect, beforeEach, afterEach, jest } from '@jest/globals';
import { WasmBackend, WasmStreamOptions } from '../../src/transport/wasm-backend.js';
import { PJSClientConfig, FrameType, Frame, PJSError } from '../../src/types/index.js';

// Mock pjs-wasm module
jest.mock('pjs-wasm', () => ({
  default: jest.fn().mockResolvedValue(undefined),
  version: jest.fn().mockReturnValue('0.1.0'),
  PriorityStream: jest.fn().mockImplementation(() => ({
    setMinPriority: jest.fn(),
    onFrame: jest.fn(),
    onComplete: jest.fn(),
    onError: jest.fn(),
    start: jest.fn(),
    free: jest.fn()
  }))
}), { virtual: true });

describe('WasmBackend Integration Tests', () => {
  let backend: WasmBackend;
  let config: Required<PJSClientConfig>;

  beforeEach(() => {
    config = {
      baseUrl: 'wasm://local',
      transport: 'wasm' as any,
      timeout: 30000,
      retry: { maxRetries: 0, baseDelay: 0, maxDelay: 0 },
      bufferSize: 1024 * 64,
      debug: false
    };

    backend = new WasmBackend(config);
  });

  afterEach(async () => {
    if (backend) {
      await backend.disconnect();
    }
  });

  describe('Initialization', () => {
    test('should initialize WASM module successfully', async () => {
      const result = await backend.connect();

      expect(result.sessionId).toBe('wasm-local');
      expect(result.supportedFeatures).toContain('wasm');
      expect(result.supportedFeatures).toContain('local-streaming');
      expect(result.supportedFeatures).toContain('priority-streaming');
      expect(backend.isWasmAvailable()).toBe(true);
    });

    test('should return same session on multiple connect calls', async () => {
      const result1 = await backend.connect();
      const result2 = await backend.connect();

      expect(result1.sessionId).toBe(result2.sessionId);
    });

    test('should report WASM version', async () => {
      await backend.connect();
      const version = backend.getWasmVersion();

      expect(version).toBe('0.1.0');
    });

    test('should handle initialization errors gracefully', async () => {
      const mockError = new Error('WASM module not found');
      const failingBackend = new WasmBackend(config);

      // Mock import to fail
      jest.spyOn(global, 'import' as any).mockRejectedValueOnce(mockError);

      await expect(failingBackend.connect()).rejects.toThrow(PJSError);
      expect(failingBackend.isWasmAvailable()).toBe(false);
    });
  });

  describe('Streaming', () => {
    beforeEach(async () => {
      await backend.connect();
    });

    test('should stream simple JSON with priority frames', async () => {
      const frames: Frame[] = [];
      backend.on('frame', (frame: Frame) => {
        frames.push(frame);
      });

      const jsonData = JSON.stringify({
        id: 123,
        name: 'Alice',
        email: 'alice@example.com',
        bio: 'Software developer'
      });

      const options: WasmStreamOptions = {
        jsonData,
        sessionId: 'test',
        streamId: 'stream-1',
        minPriority: 1
      };

      // Simulate frame emission from WASM
      const mockPriorityStream = (await import('pjs-wasm')).PriorityStream;
      const streamInstance = new mockPriorityStream();

      // Mock frame callbacks
      let frameCallback: any;
      let completeCallback: any;

      streamInstance.onFrame = jest.fn((cb: any) => {
        frameCallback = cb;
      });

      streamInstance.onComplete = jest.fn((cb: any) => {
        completeCallback = cb;
      });

      streamInstance.start = jest.fn(() => {
        // Emit skeleton frame
        frameCallback({
          type: 'skeleton',
          priority: 100,
          sequence: 0n,
          payload: JSON.stringify({ id: 123, name: null, email: null, bio: null }),
          getPayloadObject: () => ({ id: 123, name: null, email: null, bio: null })
        });

        // Emit patch frame
        frameCallback({
          type: 'patch',
          priority: 80,
          sequence: 1n,
          payload: JSON.stringify({
            patches: [
              { operation: 'set', path: '$.name', value: 'Alice' }
            ]
          }),
          getPayloadObject: () => ({
            patches: [
              { operation: 'set', path: '$.name', value: 'Alice' }
            ]
          })
        });

        // Emit complete
        completeCallback({
          totalFrames: 2,
          patchFrames: 1,
          bytesProcessed: jsonData.length,
          durationMs: 5.2
        });
      });

      await backend.startStream('test-stream', options);

      // Verify frames
      expect(frames.length).toBeGreaterThan(0);

      const skeletonFrame = frames.find(f => f.type === FrameType.Skeleton);
      expect(skeletonFrame).toBeDefined();
      expect(skeletonFrame?.priority).toBe(100);
      expect(skeletonFrame?.data).toEqual({ id: 123, name: null, email: null, bio: null });

      const patchFrame = frames.find(f => f.type === FrameType.Patch);
      expect(patchFrame).toBeDefined();
      expect(patchFrame?.priority).toBe(80);
      expect(patchFrame?.patches).toBeDefined();

      const completeFrame = frames.find(f => f.type === FrameType.Complete);
      expect(completeFrame).toBeDefined();
    });

    test('should enforce minimum priority threshold', async () => {
      const frames: Frame[] = [];
      backend.on('frame', (frame: Frame) => {
        frames.push(frame);
      });

      const jsonData = JSON.stringify({ data: 'test' });
      const options: WasmStreamOptions = {
        jsonData,
        sessionId: 'test',
        streamId: 'stream-2',
        minPriority: 50 // Only MEDIUM and above
      };

      await backend.startStream('test-stream', options);

      const mockPriorityStream = (await import('pjs-wasm')).PriorityStream;
      const streamInstance = new mockPriorityStream();

      expect(streamInstance.setMinPriority).toHaveBeenCalledWith(50);
    });

    test('should require jsonData parameter', async () => {
      const options: any = {
        sessionId: 'test',
        streamId: 'stream-3'
        // Missing jsonData
      };

      await expect(backend.startStream('test-stream', options))
        .rejects.toThrow('jsonData is required');
    });

    test('should prevent concurrent streams', async () => {
      const options: WasmStreamOptions = {
        jsonData: '{"test": true}',
        sessionId: 'test',
        streamId: 'stream-4'
      };

      await backend.startStream('stream-1', options);

      await expect(backend.startStream('stream-2', options))
        .rejects.toThrow('A stream is already active');
    });

    test('should stop active stream and cleanup resources', async () => {
      const options: WasmStreamOptions = {
        jsonData: '{"test": true}',
        sessionId: 'test',
        streamId: 'stream-5'
      };

      await backend.startStream('test-stream', options);

      const mockPriorityStream = (await import('pjs-wasm')).PriorityStream;
      const streamInstance = new mockPriorityStream();

      await backend.stopStream();

      expect(streamInstance.free).toHaveBeenCalled();
    });
  });

  describe('Error Handling', () => {
    beforeEach(async () => {
      await backend.connect();
    });

    test('should emit error on WASM streaming failure', async () => {
      const errors: PJSError[] = [];
      backend.on('error', (error: PJSError) => {
        errors.push(error);
      });

      const mockPriorityStream = (await import('pjs-wasm')).PriorityStream;
      const streamInstance = new mockPriorityStream();

      let errorCallback: any;
      streamInstance.onError = jest.fn((cb: any) => {
        errorCallback = cb;
      });

      streamInstance.start = jest.fn(() => {
        errorCallback('JSON parse error at line 5');
      });

      const options: WasmStreamOptions = {
        jsonData: 'invalid json{',
        sessionId: 'test',
        streamId: 'stream-6'
      };

      await backend.startStream('test-stream', options);

      expect(errors.length).toBeGreaterThan(0);
      expect(errors[0].message).toContain('WASM streaming error');
    });

    test('should throw error when streaming without connection', async () => {
      const disconnectedBackend = new WasmBackend(config);

      const options: WasmStreamOptions = {
        jsonData: '{"test": true}',
        sessionId: 'test',
        streamId: 'stream-7'
      };

      await expect(disconnectedBackend.startStream('test', options))
        .rejects.toThrow('WASM backend not initialized');
    });

    test('should handle malformed frame payloads', async () => {
      const errors: PJSError[] = [];
      backend.on('error', (error: PJSError) => {
        errors.push(error);
      });

      const mockPriorityStream = (await import('pjs-wasm')).PriorityStream;
      const streamInstance = new mockPriorityStream();

      let frameCallback: any;
      streamInstance.onFrame = jest.fn((cb: any) => {
        frameCallback = cb;
      });

      streamInstance.start = jest.fn(() => {
        // Emit frame with invalid payload
        try {
          frameCallback({
            type: 'skeleton',
            priority: 100,
            sequence: 0n,
            payload: 'invalid json{',
            getPayloadObject: () => {
              throw new Error('Parse failed');
            }
          });
        } catch (error) {
          // Expected to throw during frame conversion
        }
      });

      const options: WasmStreamOptions = {
        jsonData: '{"test": true}',
        sessionId: 'test',
        streamId: 'stream-8'
      };

      await expect(backend.startStream('test', options)).rejects.toThrow();
    });
  });

  describe('Memory Management', () => {
    beforeEach(async () => {
      await backend.connect();
    });

    test('should cleanup resources on disconnect', async () => {
      const options: WasmStreamOptions = {
        jsonData: '{"test": true}',
        sessionId: 'test',
        streamId: 'stream-9'
      };

      await backend.startStream('test-stream', options);

      const mockPriorityStream = (await import('pjs-wasm')).PriorityStream;
      const streamInstance = new mockPriorityStream();

      await backend.disconnect();

      expect(streamInstance.free).toHaveBeenCalled();
      expect(backend.isWasmAvailable()).toBe(false);
    });

    test('should handle disconnect without active stream', async () => {
      await expect(backend.disconnect()).resolves.not.toThrow();
    });

    test('should emit disconnect event', async () => {
      const disconnectHandler = jest.fn();
      backend.on('disconnect', disconnectHandler);

      await backend.disconnect();

      expect(disconnectHandler).toHaveBeenCalled();
    });
  });

  describe('Frame Conversion', () => {
    beforeEach(async () => {
      await backend.connect();
    });

    test('should convert WASM skeleton frame to PJS format', async () => {
      const frames: Frame[] = [];
      backend.on('frame', (frame: Frame) => {
        frames.push(frame);
      });

      const mockPriorityStream = (await import('pjs-wasm')).PriorityStream;
      const streamInstance = new mockPriorityStream();

      let frameCallback: any;
      streamInstance.onFrame = jest.fn((cb: any) => {
        frameCallback = cb;
      });

      streamInstance.start = jest.fn(() => {
        frameCallback({
          type: 'skeleton',
          priority: 100,
          sequence: 0n,
          payload: '{"id": 1}',
          getPayloadObject: () => ({ id: 1 })
        });
      });

      const options: WasmStreamOptions = {
        jsonData: '{"id": 1, "name": "test"}',
        sessionId: 'test',
        streamId: 'stream-10'
      };

      await backend.startStream('test', options);

      const skeleton = frames.find(f => f.type === FrameType.Skeleton);
      expect(skeleton).toBeDefined();
      expect(skeleton?.priority).toBe(100);
      expect(skeleton?.data).toEqual({ id: 1 });
      expect(skeleton?.metadata?.source).toBe('wasm');
      expect(skeleton?.metadata?.sequence).toBe(0);
    });

    test('should convert WASM patch frame to PJS format', async () => {
      const frames: Frame[] = [];
      backend.on('frame', (frame: Frame) => {
        frames.push(frame);
      });

      const mockPriorityStream = (await import('pjs-wasm')).PriorityStream;
      const streamInstance = new mockPriorityStream();

      let frameCallback: any;
      streamInstance.onFrame = jest.fn((cb: any) => {
        frameCallback = cb;
      });

      streamInstance.start = jest.fn(() => {
        frameCallback({
          type: 'patch',
          priority: 80,
          sequence: 1n,
          payload: JSON.stringify({
            patches: [
              { operation: 'set', path: '$.name', value: 'Alice' }
            ]
          }),
          getPayloadObject: () => ({
            patches: [
              { operation: 'set', path: '$.name', value: 'Alice' }
            ]
          })
        });
      });

      const options: WasmStreamOptions = {
        jsonData: '{"id": 1, "name": "Alice"}',
        sessionId: 'test',
        streamId: 'stream-11'
      };

      await backend.startStream('test', options);

      const patch = frames.find(f => f.type === FrameType.Patch);
      expect(patch).toBeDefined();
      expect(patch?.priority).toBe(80);
      expect(patch?.patches).toHaveLength(1);
      expect(patch?.patches?.[0].path).toBe('$.name');
      expect(patch?.metadata?.source).toBe('wasm');
    });

    test('should reject unknown frame types', async () => {
      const mockPriorityStream = (await import('pjs-wasm')).PriorityStream;
      const streamInstance = new mockPriorityStream();

      let frameCallback: any;
      streamInstance.onFrame = jest.fn((cb: any) => {
        frameCallback = cb;
      });

      streamInstance.start = jest.fn(() => {
        try {
          frameCallback({
            type: 'unknown_type',
            priority: 50,
            sequence: 0n,
            payload: '{}',
            getPayloadObject: () => ({})
          });
        } catch (error) {
          // Expected to throw
        }
      });

      const options: WasmStreamOptions = {
        jsonData: '{"test": true}',
        sessionId: 'test',
        streamId: 'stream-12'
      };

      await expect(backend.startStream('test', options)).rejects.toThrow();
    });
  });

  describe('Debug Mode', () => {
    test('should log debug messages when enabled', async () => {
      const consoleSpy = jest.spyOn(console, 'log').mockImplementation();

      const debugConfig = { ...config, debug: true };
      const debugBackend = new WasmBackend(debugConfig);

      await debugBackend.connect();

      expect(consoleSpy).toHaveBeenCalledWith(
        expect.stringContaining('[PJS WASM Backend] Initialized')
      );

      consoleSpy.mockRestore();
    });

    test('should not log debug messages when disabled', async () => {
      const consoleSpy = jest.spyOn(console, 'log').mockImplementation();

      await backend.connect();

      expect(consoleSpy).not.toHaveBeenCalled();

      consoleSpy.mockRestore();
    });
  });
});
