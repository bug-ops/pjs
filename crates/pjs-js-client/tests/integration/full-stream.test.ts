/**
 * Full Stream Integration Tests
 * 
 * End-to-end tests for complete streaming workflows.
 */

import { describe, test, expect, beforeEach, jest } from '@jest/globals';
import { PJSClient } from '../../src/core/client.js';
import { 
  Priority, 
  TransportType, 
  FrameType, 
  PJSEvent 
} from '../../src/types/index.js';

describe('Full Stream Integration', () => {
  let client: PJSClient;

  beforeEach(() => {
    client = new PJSClient({
      baseUrl: 'http://localhost:3000',
      debug: false
    });
  });

  test('should complete full streaming workflow', async () => {
    const events: string[] = [];
    const renderCalls: any[] = [];
    const progressUpdates: number[] = [];

    // Set up event listeners
    client.on(PJSEvent.Connected, () => events.push('connected'));
    client.on(PJSEvent.SkeletonReady, () => events.push('skeleton_ready'));
    client.on(PJSEvent.PatchApplied, () => events.push('patch_applied'));
    client.on(PJSEvent.StreamComplete, () => events.push('stream_complete'));

    // Mock transport for full workflow
    const mockFrames = [
      {
        type: FrameType.Skeleton as FrameType.Skeleton,
        priority: Priority.Critical,
        data: {
          user: {
            id: null,
            name: null,
            email: null,
            profile: {
              avatar: null,
              bio: null,
              settings: {
                theme: null,
                notifications: null
              }
            }
          },
          posts: [],
          analytics: {
            views: 0,
            likes: 0,
            shares: 0
          }
        },
        complete: false,
        timestamp: Date.now()
      },
      {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.Critical,
        patches: [
          { path: '$.user.id', value: 12345, operation: 'set' as const }
        ],
        timestamp: Date.now()
      },
      {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.High,
        patches: [
          { path: '$.user.name', value: 'John Doe', operation: 'set' as const },
          { path: '$.user.email', value: 'john@example.com', operation: 'set' as const }
        ],
        timestamp: Date.now()
      },
      {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.High,
        patches: [
          { path: '$.user.profile.avatar', value: 'avatar.jpg', operation: 'set' as const }
        ],
        timestamp: Date.now()
      },
      {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.Medium,
        patches: [
          { 
            path: '$.posts', 
            value: [
              { id: 1, title: 'First Post', content: 'Hello World!' },
              { id: 2, title: 'Second Post', content: 'Another post' }
            ], 
            operation: 'append' as const 
          }
        ],
        timestamp: Date.now()
      },
      {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.Low,
        patches: [
          { 
            path: '$.user.profile', 
            value: { 
              bio: 'Software developer passionate about performance',
              lastSeen: '2024-01-15T10:30:00Z'
            }, 
            operation: 'merge' as const 
          }
        ],
        timestamp: Date.now()
      },
      {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.Background,
        patches: [
          { path: '$.analytics.views', value: 1250, operation: 'set' as const },
          { path: '$.analytics.likes', value: 89, operation: 'set' as const },
          { path: '$.analytics.shares', value: 23, operation: 'set' as const }
        ],
        timestamp: Date.now()
      },
      {
        type: FrameType.Complete as FrameType.Complete,
        priority: Priority.Background,
        timestamp: Date.now(),
        checksum: 'sha256:abcd1234'
      }
    ];

    // Mock transport implementation
    jest.spyOn(client as any, 'transport', 'get').mockReturnValue({
      connect: () => Promise.resolve({ sessionId: 'test-session-123' }),
      startStream: () => {
        // Simulate frames arriving over time
        let frameIndex = 0;
        
        const sendNextFrame = () => {
          if (frameIndex < mockFrames.length) {
            const frame = mockFrames[frameIndex++];
            client.emit(PJSEvent.FrameReceived, { frame });
            
            if (frameIndex < mockFrames.length) {
              setTimeout(sendNextFrame, 50); // 50ms between frames
            }
          }
        };
        
        setTimeout(sendNextFrame, 10); // Start after 10ms
        return Promise.resolve();
      },
      on: jest.fn(),
      removeListener: jest.fn(),
      disconnect: () => Promise.resolve()
    });

    // Start streaming with callbacks
    const result = await client.stream('/api/user/profile', {
      onRender: (data, metadata) => {
        renderCalls.push({
          priority: metadata.priority,
          data: JSON.parse(JSON.stringify(data)) // Deep copy
        });
      },
      onProgress: (progress) => {
        progressUpdates.push(progress.completionPercentage);
      }
    });

    // Verify final result
    expect(result).toEqual({
      user: {
        id: 12345,
        name: 'John Doe',
        email: 'john@example.com',
        profile: {
          avatar: 'avatar.jpg',
          bio: 'Software developer passionate about performance',
          settings: {
            theme: null,
            notifications: null
          },
          lastSeen: '2024-01-15T10:30:00Z'
        }
      },
      posts: [
        { id: 1, title: 'First Post', content: 'Hello World!' },
        { id: 2, title: 'Second Post', content: 'Another post' }
      ],
      analytics: {
        views: 1250,
        likes: 89,
        shares: 23
      }
    });

    // Verify events occurred in order
    expect(events).toEqual([
      'connected',
      'skeleton_ready',
      'patch_applied',
      'patch_applied',
      'patch_applied',
      'patch_applied',
      'patch_applied',
      'patch_applied',
      'stream_complete'
    ]);

    // Verify render calls happened with correct priorities
    expect(renderCalls.length).toBeGreaterThan(0);
    expect(renderCalls[0].priority).toBe(Priority.Critical);
    
    // Critical data should be available early
    const criticalRender = renderCalls.find(call => call.priority === Priority.Critical);
    expect(criticalRender?.data.user.id).toBe(12345);

    // Progress should increase over time
    expect(progressUpdates.length).toBeGreaterThan(0);
    expect(progressUpdates[progressUpdates.length - 1]).toBe(100);

    // Verify statistics
    const stats = client.getStreamStats();
    expect(stats).toHaveLength(1);
    
    const streamStats = stats[0];
    expect(streamStats.totalFrames).toBe(8);
    expect(streamStats.priorityDistribution[Priority.Critical]).toBeGreaterThan(0);
    expect(streamStats.performance.timeToCompletion).toBeGreaterThan(0);
  });

  test('should handle priority-based progressive rendering', async () => {
    const criticalData: any[] = [];
    const highData: any[] = [];
    const mediumData: any[] = [];
    const lowData: any[] = [];

    // Mock progressive frames
    const mockFrames = [
      {
        type: FrameType.Skeleton as FrameType.Skeleton,
        priority: Priority.Critical,
        data: { status: null, user: { id: null }, content: null },
        complete: false,
        timestamp: Date.now()
      },
      {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.Critical,
        patches: [
          { path: '$.status', value: 'active', operation: 'set' as const },
          { path: '$.user.id', value: 999, operation: 'set' as const }
        ],
        timestamp: Date.now()
      },
      {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.High,
        patches: [
          { path: '$.user.name', value: 'Alice Smith', operation: 'set' as const }
        ],
        timestamp: Date.now()
      },
      {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.Medium,
        patches: [
          { path: '$.content', value: 'Lorem ipsum dolor sit amet...', operation: 'set' as const }
        ],
        timestamp: Date.now()
      },
      {
        type: FrameType.Patch as FrameType.Patch,
        priority: Priority.Low,
        patches: [
          { path: '$.metadata', value: { created: '2024-01-01' }, operation: 'set' as const }
        ],
        timestamp: Date.now()
      },
      {
        type: FrameType.Complete as FrameType.Complete,
        priority: Priority.Background,
        timestamp: Date.now()
      }
    ];

    // Mock transport
    jest.spyOn(client as any, 'transport', 'get').mockReturnValue({
      connect: () => Promise.resolve({ sessionId: 'test-session-456' }),
      startStream: () => {
        mockFrames.forEach((frame, index) => {
          setTimeout(() => {
            client.emit(PJSEvent.FrameReceived, { frame });
          }, index * 100);
        });
        return Promise.resolve();
      },
      on: jest.fn(),
      removeListener: jest.fn(),
      disconnect: () => Promise.resolve()
    });

    await client.stream('/api/data', {
      onRender: (data, metadata) => {
        const copy = JSON.parse(JSON.stringify(data));
        
        switch (metadata.priority) {
          case Priority.Critical:
            criticalData.push(copy);
            break;
          case Priority.High:
            highData.push(copy);
            break;
          case Priority.Medium:
            mediumData.push(copy);
            break;
          case Priority.Low:
            lowData.push(copy);
            break;
        }
      }
    });

    // Verify priority-based rendering
    expect(criticalData.length).toBeGreaterThan(0);
    expect(highData.length).toBeGreaterThan(0);
    expect(mediumData.length).toBeGreaterThan(0);
    expect(lowData.length).toBeGreaterThan(0);

    // Critical data should include status and user ID
    const latestCritical = criticalData[criticalData.length - 1];
    expect(latestCritical.status).toBe('active');
    expect(latestCritical.user.id).toBe(999);

    // High priority data should include user name
    const latestHigh = highData[highData.length - 1];
    expect(latestHigh.user.name).toBe('Alice Smith');

    // Medium priority should include content
    const latestMedium = mediumData[mediumData.length - 1];
    expect(latestMedium.content).toBe('Lorem ipsum dolor sit amet...');

    // Low priority should include metadata
    const latestLow = lowData[lowData.length - 1];
    expect(latestLow.metadata).toEqual({ created: '2024-01-01' });
  });

  test('should handle streaming errors gracefully', async () => {
    const errors: any[] = [];

    client.on(PJSEvent.Error, ({ error, context }) => {
      errors.push({ error: error.message, context: context.operation });
    });

    // Mock transport with error
    jest.spyOn(client as any, 'transport', 'get').mockReturnValue({
      connect: () => Promise.resolve({ sessionId: 'test-session-error' }),
      startStream: () => {
        // Send invalid frame after skeleton
        setTimeout(() => {
          const skeleton = {
            type: FrameType.Skeleton as FrameType.Skeleton,
            priority: Priority.Critical,
            data: { test: null },
            complete: false,
            timestamp: Date.now()
          };
          client.emit(PJSEvent.FrameReceived, { frame: skeleton });
        }, 10);

        setTimeout(() => {
          const invalidFrame = {
            type: 'invalid_type',
            priority: Priority.High,
            data: {}
          };
          client.emit(PJSEvent.FrameReceived, { frame: invalidFrame as any });
        }, 50);

        return Promise.resolve();
      },
      on: jest.fn(),
      removeListener: jest.fn(),
      disconnect: () => Promise.resolve()
    });

    try {
      await client.stream('/api/error-test');
    } catch (error) {
      // Stream should fail due to invalid frame
      expect(error).toBeDefined();
    }

    // Should have emitted error events
    expect(errors.length).toBeGreaterThan(0);
    expect(errors[0].error).toContain('Invalid frame');
  });

  test('should support concurrent streams', async () => {
    const stream1Results: any[] = [];
    const stream2Results: any[] = [];

    // Mock different responses for different endpoints
    jest.spyOn(client as any, 'transport', 'get').mockReturnValue({
      connect: () => Promise.resolve({ sessionId: 'concurrent-test' }),
      startStream: (endpoint: string) => {
        if (endpoint === '/api/user1') {
          setTimeout(() => {
            const skeleton = {
              type: FrameType.Skeleton as FrameType.Skeleton,
              priority: Priority.Critical,
              data: { user: { id: null, name: null } },
              complete: false,
              timestamp: Date.now()
            };
            client.emit(PJSEvent.FrameReceived, { frame: skeleton });
          }, 10);

          setTimeout(() => {
            const patch = {
              type: FrameType.Patch as FrameType.Patch,
              priority: Priority.High,
              patches: [
                { path: '$.user.id', value: 1, operation: 'set' as const },
                { path: '$.user.name', value: 'User One', operation: 'set' as const }
              ],
              timestamp: Date.now()
            };
            client.emit(PJSEvent.FrameReceived, { frame: patch });
          }, 50);

          setTimeout(() => {
            const complete = {
              type: FrameType.Complete as FrameType.Complete,
              priority: Priority.Background,
              timestamp: Date.now()
            };
            client.emit(PJSEvent.FrameReceived, { frame: complete });
          }, 100);
        }
        
        return Promise.resolve();
      },
      on: jest.fn(),
      removeListener: jest.fn(),
      disconnect: () => Promise.resolve()
    });

    // Start concurrent streams
    const promises = [
      client.stream('/api/user1', {
        onRender: (data) => stream1Results.push(JSON.parse(JSON.stringify(data)))
      }),
      client.stream('/api/user2', {
        onRender: (data) => stream2Results.push(JSON.parse(JSON.stringify(data)))
      })
    ];

    const results = await Promise.all(promises);

    expect(results).toHaveLength(2);
    expect(stream1Results.length).toBeGreaterThan(0);
    
    // Verify final data for first stream
    expect(results[0].user.id).toBe(1);
    expect(results[0].user.name).toBe('User One');
  });
});