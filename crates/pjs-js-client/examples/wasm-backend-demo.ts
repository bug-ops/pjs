/**
 * WASM Backend Demo
 *
 * Demonstrates how to use the WASM backend for client-side JSON streaming
 * without server communication.
 *
 * This example shows three integration patterns:
 * 1. PJSClient with WASM transport (high-level, full client features)
 * 2. WasmBackend directly (mid-level, transport-only)
 * 3. WasmParser streaming (low-level, parser-only)
 */

import {
  PJSClient,
  TransportType,
  createWasmBackend,
  createWasmParser,
  Priority,
  FrameType
} from '@pjs/client';

// Sample large JSON data
const sampleData = JSON.stringify({
  user: {
    id: 12345,
    name: 'Alice Johnson',
    email: 'alice@example.com',
    avatar: 'https://example.com/avatar.jpg',
    profile: {
      bio: 'Software engineer passionate about web performance',
      location: 'San Francisco, CA',
      website: 'https://alice.dev',
      social: {
        twitter: '@alicecodes',
        github: 'alicejohnson',
        linkedin: 'alicejohnson'
      }
    },
    preferences: {
      theme: 'dark',
      notifications: {
        email: true,
        push: false,
        sms: false
      },
      privacy: {
        showEmail: false,
        showLocation: true,
        allowMessages: true
      }
    }
  },
  posts: Array.from({ length: 100 }, (_, i) => ({
    id: i + 1,
    title: `Post ${i + 1}`,
    content: `Content for post ${i + 1}`.repeat(50),
    author: 'Alice Johnson',
    tags: ['technology', 'programming', 'web'],
    createdAt: new Date(2024, 0, i + 1).toISOString()
  })),
  metadata: {
    totalPosts: 100,
    version: '1.0.0',
    generated: new Date().toISOString()
  }
});

/**
 * Example 1: High-level PJSClient with WASM transport
 *
 * This is the recommended approach for most use cases. It provides
 * the full PJSClient API with all features (events, stats, etc.)
 */
async function example1_HighLevelClient() {
  console.log('\n=== Example 1: PJSClient with WASM Transport ===\n');

  const client = new PJSClient({
    baseUrl: 'wasm://local',
    transport: TransportType.WASM,
    debug: true
  });

  // Connect to WASM backend (initializes WASM module)
  await client.connect();
  console.log('WASM backend initialized');

  // Stream data with progress tracking
  const result = await client.stream('demo-data', {
    jsonData: sampleData, // WASM backend requires jsonData in options
    onRender: (partialData, metadata) => {
      console.log(`Render callback (priority ${metadata.priority}):`, {
        frameType: metadata.isComplete ? 'complete' : 'partial',
        dataKeys: Object.keys(partialData || {})
      });
    },
    onProgress: (progress) => {
      console.log(`Progress: ${progress.framesReceived} frames, priorities: [${progress.prioritiesReceived.join(', ')}]`);
    }
  });

  console.log('\nFinal result keys:', Object.keys(result));
  console.log('User name:', result.user?.name);
  console.log('Total posts:', result.posts?.length);

  await client.disconnect();
}

/**
 * Example 2: Mid-level WasmBackend transport
 *
 * Direct use of WasmBackend for custom streaming logic.
 * Good for advanced use cases or integration with custom frameworks.
 */
async function example2_DirectBackend() {
  console.log('\n=== Example 2: Direct WasmBackend Transport ===\n');

  const backend = await createWasmBackend({
    baseUrl: 'wasm://local',
    transport: TransportType.WASM,
    debug: false,
    timeout: 30000,
    bufferSize: 1024 * 1024,
    priorityThreshold: Priority.Background,
    maxConcurrentStreams: 10
  });

  console.log('WASM version:', backend.getWasmVersion());
  console.log('WASM available:', backend.isWasmAvailable());

  let frameCount = 0;

  // Listen for frames
  backend.on('frame', (frame) => {
    frameCount++;
    console.log(`Frame ${frameCount}:`, {
      type: frame.type,
      priority: frame.priority,
      hasData: frame.type === FrameType.Skeleton ? !!frame.data : undefined,
      patchCount: frame.type === FrameType.Patch ? frame.patches?.length : undefined
    });
  });

  // Start streaming
  await backend.startStream('demo', {
    sessionId: 'wasm-local',
    streamId: 'stream-1',
    jsonData: sampleData,
    minPriority: 25 // Only frames with priority >= 25
  });

  console.log(`\nTotal frames received: ${frameCount}`);

  await backend.disconnect();
}

/**
 * Example 3: Low-level WasmParser streaming
 *
 * Direct parser API for maximum control.
 * Use when you need just parsing without transport layer.
 */
async function example3_ParserOnly() {
  console.log('\n=== Example 3: WasmParser Streaming API ===\n');

  const parser = await createWasmParser({ debug: false });

  console.log('Parser implementation:', parser.getImplementation());

  const frames: any[] = [];

  await parser.stream(
    sampleData,
    {
      onFrame: (frame) => {
        frames.push(frame);
        console.log(`Frame ${frames.length}:`, {
          type: frame.type,
          priority: frame.priority,
          sequence: frame.metadata?.sequence
        });
      },
      onComplete: (stats) => {
        console.log('\nStream complete:', {
          totalFrames: stats.totalFrames,
          patchFrames: stats.patchFrames,
          durationMs: `${stats.durationMs.toFixed(2)}ms`
        });
      },
      onError: (error) => {
        console.error('Stream error:', error);
      }
    },
    50 // Min priority: 50 (Medium and above only)
  );

  console.log(`\nTotal frames collected: ${frames.length}`);

  parser.dispose();
}

/**
 * Example 4: Feature detection with fallback
 *
 * Shows how to gracefully handle WASM unavailability.
 */
async function example4_FeatureDetection() {
  console.log('\n=== Example 4: Feature Detection with Fallback ===\n');

  const parser = await createWasmParser({
    debug: true,
    preferNative: false // Try WASM first
  });

  if (parser.isWasmAvailable()) {
    console.log('Using WASM backend for streaming');
    await parser.stream(sampleData, {
      onFrame: (frame) => console.log('WASM frame:', frame.type),
      onComplete: () => console.log('WASM stream complete')
    });
  } else {
    console.log('WASM not available, falling back to native JSON.parse()');
    const result = parser.parse(sampleData);
    console.log('Parsed with native (no streaming):', Object.keys(result));
  }

  parser.dispose();
}

/**
 * Example 5: Performance comparison
 *
 * Compare WASM streaming vs native parsing.
 */
async function example5_PerformanceComparison() {
  console.log('\n=== Example 5: Performance Comparison ===\n');

  const parser = await createWasmParser({ debug: false });

  // WASM streaming
  const wasmStart = performance.now();
  await parser.stream(sampleData, {
    onFrame: () => {}, // No-op
    onComplete: () => {}
  });
  const wasmDuration = performance.now() - wasmStart;

  // Native parsing
  const nativeStart = performance.now();
  const result = JSON.parse(sampleData);
  const nativeDuration = performance.now() - nativeStart;

  console.log('Results:');
  console.log(`  WASM streaming: ${wasmDuration.toFixed(2)}ms`);
  console.log(`  Native parse:   ${nativeDuration.toFixed(2)}ms`);
  console.log(`  Difference:     ${(wasmDuration - nativeDuration).toFixed(2)}ms`);
  console.log(`\nNote: WASM has overhead for small JSON. Benefits appear with large data (>100KB)`);

  parser.dispose();
}

// Run all examples
async function runAllExamples() {
  console.log('PJS WASM Backend Examples');
  console.log('=========================\n');
  console.log('Sample data size:', (sampleData.length / 1024).toFixed(1), 'KB\n');

  try {
    await example1_HighLevelClient();
    await example2_DirectBackend();
    await example3_ParserOnly();
    await example4_FeatureDetection();
    await example5_PerformanceComparison();

    console.log('\n=== All examples completed successfully ===\n');
  } catch (error) {
    console.error('\nExample failed:', error);
    process.exit(1);
  }
}

// Run if executed directly
if (require.main === module) {
  runAllExamples().catch((error) => {
    console.error('Fatal error:', error);
    process.exit(1);
  });
}

export {
  example1_HighLevelClient,
  example2_DirectBackend,
  example3_ParserOnly,
  example4_FeatureDetection,
  example5_PerformanceComparison
};
