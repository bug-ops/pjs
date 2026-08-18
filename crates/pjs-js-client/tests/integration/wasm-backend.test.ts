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

import { describe, test, expect, beforeEach, afterEach } from '@jest/globals';
import { WasmBackend, WasmStreamOptions } from '../../src/transport/wasm-backend.js';
import { PJSClientConfig, FrameType, Frame, PJSError, Priority } from '../../src/types/index.js';
import { loadWasmModule } from '../../src/utils/wasm-loader.js';
import * as wasmLoader from '../../src/utils/wasm-loader.js';
import { describeWasmPkg, wasmPkgVersion } from './wasm-pkg.js';

describeWasmPkg('WasmBackend Integration Tests', () => {
  let backend: WasmBackend;
  let config: Required<PJSClientConfig>;
  let restorePriorityStream: (() => void) | null = null;

  // `loadWasmModule()` returns a cached singleton module object, and
  // `WasmBackend.startStream()` reads `PriorityStream` off it fresh on every
  // call (not a value captured at `connect()` time) — so replacing the
  // property in place lets a mock reach the instance the backend actually
  // constructs internally, unlike getting a class reference from a separate
  // `import('pjs-wasm')`.
  async function installMockPriorityStream(): Promise<any> {
    const wasmModule = (await loadWasmModule()) as any;
    const instance: any = {
      setMinPriority: jest.fn(),
      onFrame: jest.fn(),
      onComplete: jest.fn(),
      onError: jest.fn(),
      start: jest.fn(),
      free: jest.fn()
    };
    // Capture the true original only on the first install of a test — a
    // second call in the same test (not currently exercised, but should
    // stay safe) must not overwrite it with the previous mock, or restore()
    // would leave the mock in place instead of the real PriorityStream.
    if (!restorePriorityStream) {
      const original = wasmModule.PriorityStream;
      restorePriorityStream = () => {
        wasmModule.PriorityStream = original;
        restorePriorityStream = null;
      };
    }
    wasmModule.PriorityStream = jest.fn(() => instance);
    return instance;
  }

  beforeEach(() => {
    config = {
      baseUrl: 'wasm://local',
      transport: 'wasm' as any,
      sessionId: '',
      headers: {},
      timeout: 30000,
      bufferSize: 1024 * 64,
      priorityThreshold: Priority.Background,
      maxConcurrentStreams: 10,
      debug: false
    };

    backend = new WasmBackend(config);
  });

  afterEach(async () => {
    try {
      if (backend) {
        await backend.disconnect();
      }
    } finally {
      if (restorePriorityStream) {
        restorePriorityStream();
      }
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

      expect(version).toBe(wasmPkgVersion);
    });

    test('should handle initialization errors gracefully', async () => {
      const mockError = new Error('WASM module not found');
      const failingBackend = new WasmBackend(config);

      const loadSpy = jest.spyOn(wasmLoader, 'loadWasmModule').mockRejectedValueOnce(mockError);

      try {
        await expect(failingBackend.connect()).rejects.toThrow(PJSError);
        expect(failingBackend.isWasmAvailable()).toBe(false);
      } finally {
        loadSpy.mockRestore();
      }
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
      const streamInstance = await installMockPriorityStream();

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

      const skeletonFrame = frames.find(f => f.frame_type === FrameType.Skeleton);
      expect(skeletonFrame).toBeDefined();
      expect(skeletonFrame?.priority).toBe(100);
      expect(skeletonFrame?.payload).toEqual({ id: 123, name: null, email: null, bio: null });

      const patchFrame = frames.find(f => f.frame_type === FrameType.Patch);
      expect(patchFrame).toBeDefined();
      expect(patchFrame?.priority).toBe(80);
      expect(patchFrame?.payload).toBeDefined();

      const completeFrame = frames.find(f => f.frame_type === FrameType.Complete);
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

      const streamInstance = await installMockPriorityStream();

      await backend.startStream('test-stream', options);

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

      const streamInstance = await installMockPriorityStream();

      await backend.startStream('test-stream', options);

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

      const streamInstance = await installMockPriorityStream();

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

      const streamInstance = await installMockPriorityStream();

      let frameCallback: any;
      streamInstance.onFrame = jest.fn((cb: any) => {
        frameCallback = cb;
      });

      streamInstance.start = jest.fn(() => {
        // Emit frame with invalid payload — convertWasmFrame's JSON.parse
        // throws synchronously here; let it propagate so startStream's own
        // try/catch converts it to a PJSError and rejects, instead of being
        // swallowed by this mock. NOTE: this asserts plain-JS exception
        // propagation through a mocked PriorityStream. The real binding
        // invokes onFrame from Rust across the wasm boundary and may not
        // propagate the same way — see the TODO(follow-up) on the skipped
        // streaming test in wasm-parser.test.ts, where a callback-internal
        // side effect (stream.free()) is silently swallowed instead of
        // throwing catchably.
        frameCallback({
          type: 'skeleton',
          priority: 100,
          sequence: 0n,
          payload: 'invalid json{',
          getPayloadObject: () => {
            throw new Error('Parse failed');
          }
        });
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

      const streamInstance = await installMockPriorityStream();

      await backend.startStream('test-stream', options);

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

      const streamInstance = await installMockPriorityStream();

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

      const skeleton = frames.find(f => f.frame_type === FrameType.Skeleton);
      expect(skeleton).toBeDefined();
      expect(skeleton?.priority).toBe(100);
      expect(skeleton?.payload).toEqual({ id: 1 });
      expect(skeleton?.metadata?.source).toBe('wasm');
      expect(skeleton?.sequence).toBe(0);
    });

    test('should convert WASM patch frame to PJS format', async () => {
      const frames: Frame[] = [];
      backend.on('frame', (frame: Frame) => {
        frames.push(frame);
      });

      const streamInstance = await installMockPriorityStream();

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

      const patch = frames.find(f => f.frame_type === FrameType.Patch);
      expect(patch).toBeDefined();
      expect(patch?.priority).toBe(80);
      expect(patch?.payload.patches).toHaveLength(1);
      expect(patch?.payload.patches[0].path).toBe('$.name');
      expect(patch?.metadata?.source).toBe('wasm');
    });

    test('should reject unknown frame types', async () => {
      const streamInstance = await installMockPriorityStream();

      let frameCallback: any;
      streamInstance.onFrame = jest.fn((cb: any) => {
        frameCallback = cb;
      });

      streamInstance.start = jest.fn(() => {
        // mapFrameType throws synchronously here; let it propagate so
        // startStream's own try/catch converts it to a PJSError and rejects.
        // NOTE: mock-only propagation semantics — see the comment on the
        // equivalent case in "should handle malformed frame payloads" above.
        frameCallback({
          type: 'unknown_type',
          priority: 50,
          sequence: 0n,
          payload: '{}',
          getPayloadObject: () => ({})
        });
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
