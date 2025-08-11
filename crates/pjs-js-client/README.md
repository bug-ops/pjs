# PJS JavaScript/TypeScript Client

[![npm version](https://badge.fury.io/js/%40pjs%2Fclient.svg)](https://www.npmjs.com/package/@pjs/client)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/pjs-rs/pjs)
[![TypeScript](https://img.shields.io/badge/TypeScript-Ready-blue.svg)](https://www.typescriptlang.org/)

**Progressive JSON Loading for JavaScript & TypeScript Applications**

The official JavaScript/TypeScript client library for the Priority JSON Streaming Protocol (PJS). Deliver JSON data progressively with intelligent prioritization for optimal user experience and performance.

## ✨ Features

- 🚀 **Progressive Loading**: Render critical data first, details later
- ⚡ **Priority-Based Streaming**: Critical fields arrive in milliseconds
- 🎯 **Smart Reconstruction**: Automatic JSON patching and assembly
- 🌐 **Multiple Transports**: HTTP, WebSocket, Server-Sent Events
- 🛡️ **Type Safety**: Full TypeScript support with comprehensive types
- 📊 **Performance Metrics**: Built-in monitoring and statistics
- 🎨 **React Integration**: (Coming soon) Hooks and components
- 📱 **Framework Agnostic**: Works with any JavaScript framework

## 🏃‍♂️ Quick Start

### Installation

```bash
npm install @pjs/client
# or
yarn add @pjs/client
# or
pnpm add @pjs/client
```

### Basic Usage

```typescript
import { PJSClient, Priority } from '@pjs/client';

// Create client
const client = new PJSClient({
  baseUrl: 'http://localhost:3000'
});

// Stream JSON with progressive rendering
const data = await client.stream('/api/users', {
  onRender: (partialData, metadata) => {
    // Render immediately for high-priority data
    if (metadata.priority >= Priority.High) {
      updateUI(partialData);
    }
  },
  onProgress: (progress) => {
    console.log(`Loading: ${progress.completionPercentage}%`);
  }
});

console.log('Complete data:', data);
```

### Quick Start Helper

```typescript
import { quickStart } from '@pjs/client';

// Minimal setup with smart defaults
const client = quickStart('http://localhost:3000');
const userData = await client.stream('/api/user/profile');
```

## 📚 API Reference

### PJSClient

Main client class for streaming JSON data.

```typescript
class PJSClient {
  constructor(config: PJSClientConfig)
  
  async connect(): Promise<string>
  async disconnect(): Promise<void>
  async stream<T>(endpoint: string, options?: StreamOptions): Promise<T>
  
  getSessionId(): string | undefined
  getStreamStats(): StreamStats[]
  isClientConnected(): boolean
  
  // Event listeners
  on(event: PJSEvent, listener: EventListener): this
}
```

#### Configuration

```typescript
interface PJSClientConfig {
  baseUrl: string                    // PJS server URL
  transport?: TransportType          // 'http' | 'websocket' | 'sse'
  sessionId?: string                 // Existing session ID
  headers?: Record<string, string>   // Custom headers
  timeout?: number                   // Request timeout (default: 30000)
  debug?: boolean                    // Enable debug logging
  bufferSize?: number                // Buffer size (default: 1MB)
  priorityThreshold?: Priority       // Minimum priority to process
  maxConcurrentStreams?: number      // Max concurrent streams
}
```

#### Stream Options

```typescript
interface StreamOptions {
  priorityStrategy?: PriorityStrategy  // Custom prioritization
  onRender?: RenderCallback           // Progressive rendering
  onProgress?: ProgressCallback       // Progress updates
  timeout?: number                    // Stream timeout
  queryParams?: Record<string, string>
  headers?: Record<string, string>
}
```

### Priority System

```typescript
enum Priority {
  Critical = 100,    // User identity, errors, status
  High = 75,         // Names, titles, key information
  Medium = 50,       // Descriptions, content
  Low = 25,          // Metadata, timestamps
  Background = 10    // Analytics, large datasets
}
```

### Event System

```typescript
// Listen for events
client.on('skeleton_ready', ({ data, processingTime }) => {
  console.log('Initial structure ready:', data);
});

client.on('patch_applied', ({ patch, priority, path }) => {
  console.log(`Applied ${patch.operation} to ${path}`);
});

client.on('stream_complete', ({ data, stats, totalTime }) => {
  console.log(`Stream completed in ${totalTime}ms`);
  console.log('Final data:', data);
  console.log('Statistics:', stats);
});

client.on('progress_update', (progress) => {
  console.log(`Progress: ${progress.completionPercentage}%`);
});
```

## 🌐 Transport Options

### HTTP Transport (Default)

Best for: Simple integration, maximum compatibility

```typescript
const client = new PJSClient({
  baseUrl: 'http://localhost:3000',
  transport: 'http'
});
```

### WebSocket Transport

Best for: Real-time applications, bi-directional communication

```typescript
const client = new PJSClient({
  baseUrl: 'ws://localhost:3000',
  transport: 'websocket'
});
```

### Server-Sent Events

Best for: One-way streaming, automatic reconnection

```typescript
const client = new PJSClient({
  baseUrl: 'http://localhost:3000',
  transport: 'sse'
});
```

## 🎨 Advanced Examples

### React Integration

```typescript
import React, { useState, useEffect } from 'react';
import { PJSClient, Priority } from '@pjs/client';

function UserProfile({ userId }: { userId: string }) {
  const [userData, setUserData] = useState(null);
  const [loading, setLoading] = useState(true);
  
  useEffect(() => {
    const client = new PJSClient({
      baseUrl: process.env.REACT_APP_API_URL
    });
    
    client.stream(`/api/users/${userId}`, {
      onRender: (partialData, metadata) => {
        // Update UI progressively
        setUserData(partialData);
        
        // Stop loading spinner when critical data arrives
        if (metadata.priority >= Priority.High) {
          setLoading(false);
        }
      }
    });
  }, [userId]);
  
  if (loading) return <div>Loading...</div>;
  
  return (
    <div>
      <h1>{userData?.name || 'Loading name...'}</h1>
      <p>{userData?.email || 'Loading email...'}</p>
      {/* More fields will populate progressively */}
    </div>
  );
}
```

### Custom Priority Strategy

```typescript
const customStrategy: PriorityStrategy = {
  name: 'ecommerce',
  calculatePriority: (path, value, context) => {
    if (path.includes('price')) return Priority.Critical;
    if (path.includes('product.name')) return Priority.High;
    if (path.includes('reviews')) return Priority.Background;
    return Priority.Medium;
  }
};

const data = await client.stream('/api/products', {
  priorityStrategy: customStrategy
});
```

### Performance Monitoring

```typescript
client.on('stream_complete', ({ stats }) => {
  console.log('Performance Metrics:');
  console.log(`Time to first frame: ${stats.performance.timeToFirstFrame}ms`);
  console.log(`Time to skeleton: ${stats.performance.timeToSkeleton}ms`);
  console.log(`Total time: ${stats.performance.timeToCompletion}ms`);
  console.log(`Throughput: ${stats.performance.throughputMbps} MB/s`);
  console.log(`Memory efficiency: ${stats.performance.memoryStats.efficiency}%`);
});
```

### Error Handling

```typescript
try {
  const data = await client.stream('/api/data');
} catch (error) {
  if (error instanceof PJSError) {
    switch (error.type) {
      case PJSErrorType.ConnectionError:
        console.error('Connection failed:', error.message);
        break;
      case PJSErrorType.TimeoutError:
        console.error('Stream timed out:', error.message);
        break;
      case PJSErrorType.ParseError:
        console.error('Invalid data received:', error.message);
        break;
    }
  }
}
```

## 🔧 Utilities

### Frame Validation

```typescript
import { validateFrame } from '@pjs/client';

const isValidFrame = validateFrame(receivedData);
```

### JSON Path Parsing

```typescript
import { parseJsonPath } from '@pjs/client';

const result = parseJsonPath('$.user.profile[0].name');
console.log(result.segments); // ['user', 'profile[0]', 'name']
console.log(result.isValid);  // true
```

### Memory Estimation

```typescript
import { estimateObjectSize, formatBytes } from '@pjs/client';

const size = estimateObjectSize(largeObject);
console.log(`Object size: ${formatBytes(size)}`);
```

## 🚀 Performance Benefits

### Traditional JSON Loading
```
Time →  |-------- 2000ms --------| 
Data →  [                    ████] ← All data at once
UI   →  [          loading...    ] ← User waits
```

### PJS Progressive Loading
```
Time →  |-------- 2000ms --------| 
Data →  [██]    [██]    [██]   [█] ← Progressive chunks
UI   →  [█]     [██]    [███] [██] ← Immediate rendering
        ↑        ↑       ↑     ↑
      50ms     200ms    500ms  2000ms
   Critical    High    Medium   Low
```

**Results:**
- ⚡ **70% faster** perceived loading time
- 🎯 **Critical data** in 50ms vs 2000ms  
- 📱 **Better UX** on slow connections
- 💾 **Lower memory** usage during loading

## 🛠️ Development

### Building

```bash
npm install
npm run build
```

### Testing

```bash
npm test
npm run test:watch
npm run test:coverage
```

### Linting

```bash
npm run lint
npm run lint:fix
```

### Documentation

```bash
npm run docs
```

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guide](../CONTRIBUTING.md) for details.

### Development Setup

```bash
git clone https://github.com/pjs-rs/pjs
cd pjs/crates/pjs-js-client
npm install
npm run dev
```

## 📋 Requirements

- **Node.js**: 18.0.0 or higher
- **TypeScript**: 5.0+ (for TypeScript projects)
- **Modern Browser**: ES2020 support required

## 📄 License

Licensed under either of:
- **Apache License, Version 2.0** ([LICENSE-APACHE](../../LICENSE-APACHE))
- **MIT License** ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## 🔗 Related Projects

- **[pjs-rs](../pjs-core)** - Rust implementation and core protocol
- **[PJS Server Examples](../../examples)** - Server implementations
- **[PJS Benchmarks](../pjs-bench)** - Performance comparisons

---

<div align="center">

**[Documentation](https://docs.pjs.rs) • [Examples](../../examples) • [Benchmarks](../pjs-bench) • [GitHub](https://github.com/pjs-rs/pjs)**

Made with ❤️ for the JavaScript community

</div>