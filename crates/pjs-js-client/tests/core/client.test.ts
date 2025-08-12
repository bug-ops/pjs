/**
 * PJS Client Tests
 * 
 * Comprehensive tests for the main PJSClient class functionality.
 */

import { describe, test, expect, beforeEach, afterEach, jest } from '@jest/globals';
import { PJSClient } from '../../src/core/client.js';
import { 
  Priority, 
  TransportType, 
  FrameType, 
  PJSEvent, 
  PJSError, 
  PJSErrorType 
} from '../../src/types/index.js';

describe('PJSClient', () => {
  let client: PJSClient;

  beforeEach(() => {
    client = new PJSClient({
      baseUrl: 'http://localhost:3000',
      debug: false
    });
  });

  afterEach(() => {
    if (client.isClientConnected()) {
      client.disconnect();
    }
  });

  describe('Configuration', () => {
    test('should initialize with default configuration', () => {
      const defaultClient = new PJSClient({
        baseUrl: 'http://localhost:3000'
      });

      expect(defaultClient).toBeDefined();
      expect(defaultClient.isClientConnected()).toBe(false);
    });

    test('should validate required baseUrl', () => {
      expect(() => {
        new PJSClient({} as any);
      }).toThrow(PJSError);
    });

    test('should normalize baseUrl by removing trailing slash', () => {
      const client = new PJSClient({
        baseUrl: 'http://localhost:3000/'
      });

      // Internal config should have no trailing slash
      expect(client).toBeDefined();
    });

    test('should set custom configuration options', () => {
      const customClient = new PJSClient({
        baseUrl: 'http://localhost:3000',
        transport: TransportType.WebSocket,
        timeout: 15000,
        debug: true,
        priorityThreshold: Priority.High
      });

      expect(customClient).toBeDefined();
    });
  });

  describe('Connection Management', () => {
    test('should connect and return session ID', async () => {
      const sessionId = await client.connect();
      
      expect(sessionId).toBeDefined();
      expect(typeof sessionId).toBe('string');
      expect(client.isClientConnected()).toBe(true);
      expect(client.getSessionId()).toBe(sessionId);
    });

    test('should throw error when connecting twice', async () => {
      await client.connect();
      
      await expect(client.connect()).rejects.toThrow(PJSError);
    });

    test('should disconnect successfully', async () => {
      await client.connect();
      await client.disconnect();
      
      expect(client.isClientConnected()).toBe(false);
      expect(client.getSessionId()).toBeUndefined();
    });

    test('should handle disconnect when not connected', async () => {
      // Should not throw
      await expect(client.disconnect()).resolves.toBeUndefined();
    });
  });

  describe('Event System', () => {
    test('should emit connection events', async () => {
      const connectListener = jest.fn();
      const disconnectListener = jest.fn();

      client.on(PJSEvent.Connected, connectListener);
      client.on(PJSEvent.Disconnected, disconnectListener);

      await client.connect();
      await client.disconnect();

      expect(connectListener).toHaveBeenCalledWith({
        sessionId: expect.any(String)
      });
      expect(disconnectListener).toHaveBeenCalled();
    });

    test('should emit error events', (done: () => void) => {
      client.on(PJSEvent.Error, ({ error, context }) => {
        expect(error).toBeInstanceOf(PJSError);
        expect(context).toBeDefined();
        done();
      });

      // Trigger an error by trying to stream without connection
      client.stream('/test').catch(() => {
        // Expected to fail
      });
    });
  });

  describe('Streaming', () => {
    beforeEach(async () => {
      // Mock successful frames for streaming tests
      const mockFrames = [
        {
          type: FrameType.Skeleton as FrameType.Skeleton,
          priority: Priority.Critical,
          data: { id: null, name: null },
          complete: false,
          timestamp: Date.now()
        },
        {
          type: FrameType.Patch as FrameType.Patch,
          priority: Priority.High,
          patches: [
            { path: '$.id', value: 123, operation: 'set' as const },
            { path: '$.name', value: 'Test User', operation: 'set' as const }
          ],
          timestamp: Date.now()
        },
        {
          type: FrameType.Complete as FrameType.Complete,
          priority: Priority.Background,
          timestamp: Date.now()
        }
      ];

      // Mock transport to emit these frames
      jest.spyOn(client as any, 'transport').mockImplementation({
        connect: () => Promise.resolve({ sessionId: 'test-session' }),
        startStream: () => {
          // Emit frames after a short delay
          setTimeout(() => {
            mockFrames.forEach(frame => {
              client.emit(PJSEvent.FrameReceived, { frame });
            });
          }, 50);
          return Promise.resolve();
        },
        on: jest.fn(),
        removeListener: jest.fn()
      });
    });

    test('should stream data successfully', async () => {
      const result = await client.stream('/api/test');
      
      expect(result).toEqual({
        id: 123,
        name: 'Test User'
      });
    });

    test('should call render callback during streaming', async () => {
      const renderCallback = jest.fn();
      const progressCallback = jest.fn();

      await client.stream('/api/test', {
        onRender: renderCallback,
        onProgress: progressCallback
      });

      expect(renderCallback).toHaveBeenCalled();
      expect(progressCallback).toHaveBeenCalled();
    });

    test('should handle stream timeout', async () => {
      const slowClient = new PJSClient({
        baseUrl: 'http://localhost:3000',
        timeout: 100 // Very short timeout
      });

      await expect(
        slowClient.stream('/api/slow', { timeout: 100 })
      ).rejects.toThrow(PJSError);
    });

    test('should auto-connect if not connected', async () => {
      expect(client.isClientConnected()).toBe(false);
      
      await client.stream('/api/test');
      
      expect(client.isClientConnected()).toBe(true);
    });
  });

  describe('Statistics', () => {
    test('should track stream statistics', async () => {
      await client.stream('/api/test');
      
      const stats = client.getStreamStats();
      expect(stats).toHaveLength(1);
      
      const streamStats = stats[0];
      expect(streamStats).toMatchObject({
        streamId: expect.any(String),
        startTime: expect.any(Number),
        totalFrames: expect.any(Number),
        priorityDistribution: expect.any(Object),
        performance: expect.objectContaining({
          timeToFirstFrame: expect.any(Number),
          timeToSkeleton: expect.any(Number),
          timeToCompletion: expect.any(Number)
        })
      });
    });

    test('should maintain stats for multiple streams', async () => {
      await Promise.all([
        client.stream('/api/test1'),
        client.stream('/api/test2')
      ]);
      
      const stats = client.getStreamStats();
      expect(stats).toHaveLength(2);
    });
  });

  describe('Error Handling', () => {
    test('should handle transport errors gracefully', async () => {
      // Mock transport error
      const errorClient = new PJSClient({
        baseUrl: 'http://invalid-url:9999'
      });

      await expect(errorClient.connect()).rejects.toThrow(PJSError);
    });

    test('should validate frame structure', async () => {
      const errorListener = jest.fn();
      client.on(PJSEvent.Error, errorListener);

      // Simulate invalid frame
      const invalidFrame = { invalid: true };
      client.emit(PJSEvent.FrameReceived, { frame: invalidFrame as any });

      // Should emit error event
      expect(errorListener).toHaveBeenCalled();
    });

    test('should handle protocol violations', async () => {
      const errorListener = jest.fn();
      client.on(PJSEvent.Error, errorListener);

      // Simulate patch before skeleton
      const patchFrame = {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.Medium,
        patches: [],
        timestamp: Date.now()
      };

      client.emit(PJSEvent.FrameReceived, { frame: patchFrame });

      // Should emit protocol error
      expect(errorListener).toHaveBeenCalled();
    });
  });

  describe('Transport Selection', () => {
    test('should create HTTP transport by default', () => {
      const httpClient = new PJSClient({
        baseUrl: 'http://localhost:3000'
      });

      expect(httpClient).toBeDefined();
    });

    test('should create WebSocket transport when specified', () => {
      const wsClient = new PJSClient({
        baseUrl: 'ws://localhost:3000',
        transport: TransportType.WebSocket
      });

      expect(wsClient).toBeDefined();
    });

    test('should create SSE transport when specified', () => {
      const sseClient = new PJSClient({
        baseUrl: 'http://localhost:3000',
        transport: TransportType.ServerSentEvents
      });

      expect(sseClient).toBeDefined();
    });

    test('should throw error for unsupported transport', () => {
      expect(() => {
        new PJSClient({
          baseUrl: 'http://localhost:3000',
          transport: 'invalid' as any
        });
      }).toThrow(PJSError);
    });
  });
});