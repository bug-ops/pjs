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
- 🌐 **Multiple Transports**: WebSocket, plus HTTP and Server-Sent Events (**experimental**, see [Transport Options](#-transport-options))
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
  data?: JsonValue                    // Document to stream (required by the WebSocket transport)
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

client.on('server_shutdown', () => {
  console.log('Server is shutting down; reconnect later');
});
```

A stopping server ends a stream with a shutdown marker: the named SSE event
`shutdown` (data `{"event":"shutdown"}`) or, for HTTP, the terminal NDJSON line
`{"event":"shutdown"}`. The client emits `server_shutdown`, then `disconnected`
with reason `server shutdown`, never reconnects on its own, and rejects a
pending `stream()` that has not completed with a `PJSError` of type
`SERVER_SHUTDOWN`.

## 🌐 Transport Options

The WebSocket transport works against a stock `pjs-core` WebSocket server (see
[WebSocket Transport](#websocket-transport)).

**Experimental:** the HTTP and SSE transports speak the wire format described
below, which an external server must implement. The routes and payloads shipped
with `pjs-core` do not match it, so these transports do not work against a
stock `pjs-core` server. Known near-misses:

- `HttpTransport.connect` calls the real `POST /pjs/sessions`, but sends
  `client_info` as an object while the server expects a string; the server
  answers 422.

### Wire format

| Transport | Requests | Responses |
|-----------|----------|-----------|
| HTTP | `POST /pjs/sessions` with `{"client_info":{"type":"javascript","version":"<client version>"}}`, then `GET /pjs/stream{endpoint}?session_id&stream_id&<queryParams>` with `Accept: application/x-ndjson` | `{"session_id","supported_features"?}`; one Frame per NDJSON line, optional terminal `{"event":"shutdown"}` |
| SSE | `GET /pjs/stream{endpoint}/sse?session_id&stream_id&<queryParams>` | `message`/`frame` events carry Frame data; `shutdown` event; `error` event with `{"message"}` |

A Frame is the `pjs-core` Frame JSON (`frame_type`, `priority`, `payload`, ...)
that the client validates.

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

The client uploads the document and the server streams it back by priority, so
`StreamOptions.data` is required (`null` is a valid document):

```typescript
const client = new PJSClient({
  baseUrl: 'ws://localhost:3000',
  transport: 'websocket'
});

const document = await client.stream('/ignored', { data: { id: 1, name: 'Ada' } });
```

How it behaves:

- It connects to `{baseUrl}/ws` (`http`/`https` map to `ws`/`wss`) and speaks the
  `pjs-core` `WsMessage` protocol with patch framing. The `endpoint` argument is
  ignored.
- One stream at a time: starting a second stream cancels the first, and the
  first `stream()` rejects with `STREAM_ERROR` (as does `stopStream()`).
  `disconnect()` rejects a pending stream with a `ConnectionError`. There is no
  automatic reconnection; a socket that closes mid-stream rejects with a
  `ConnectionError`.
- A server `Error` reply rejects with a `ProtocolError` whose `context` is
  `{ code, error }`. A document over the server's inbound cap (1 MiB by
  default) makes the server close the socket, so the stream rejects with a
  `ConnectionError` and no code; 413 comes only from plan limits. A document
  nested deeper than the server's JSON recursion limit is answered with 400,
  not the 413 `PathTooDeep` that spec 066 describes (#737).
- The server's 400 for an undecodable message carries no session id, so it is
  attributed to the active stream; after a superseded `StreamInit` it can fail
  the stream that replaced it.
- The reconstructed object has the server's **sorted** key order, not the input
  order.
- It needs `globalThis.WebSocket` (browsers and Node 22+; on older Node assign
  one from the `ws` package) and `globalThis.crypto.randomUUID`; without
  `randomUUID`, `stream()` rejects with a `ConfigurationError`.
- `data` is typed `JsonValue`. A document declared as an `interface` is not
  assignable to it (interfaces lack an index signature); declare it with a
  `type` alias or annotate it once:

  ```typescript
  const payload: JsonValue = { id: 1, tags: ['a'] };
  ```

Server requirements (Rust side):

- Serve the router with `into_make_service_with_connect_info::<SocketAddr>()`;
  without connect info every upgrade is answered with 500.
- Browsers always send an `Origin` header, and the default origin policy rejects
  any upgrade that carries one with 403. Build the server with
  `AxumWebSocketTransport::new().with_allowed_origins(vec![...])` to accept
  browser clients. Node's `ws` sends no `Origin` by default, so it connects to
  a stock server.

**Breaking changes (WebSocket):**

- The old `start_stream`/`stop_stream` protocol and the `/pjs/ws` path are gone;
  the transport talks to `/ws`.
- `StreamOptions.data` is required for the WebSocket transport.
- Automatic reconnection was removed.
- `connect()` now advertises the feature `patch-framing` instead of
  `realtime`/`multiplexing`.
- `PatchOperation.operation` gains `append_str`, so exhaustive switches over the
  operation need a new case; `PATCH_OPERATIONS` lists all operations.
- A root-path `append` now pushes onto an array root instead of being skipped.

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

A path is `$` followed by `.key` or `[n]` segments (`$`, `$[0].a`, `$.m[0][1]`, `$.user-id`); keys are non-empty and contain no `.`, `[` or `]`, and indices are non-negative safe integers.

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

### Verify (matches CI)

```bash
npm ci && npm run typecheck && npm run lint && npm test
```

Run `npm ci` first: a stale `node_modules` fails lint (for example `Cannot find package '@eslint/js'`) while CI, which installs from the lockfile, passes.

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
git clone https://github.com/bug-ops/pjs
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

- **[pjson-rs](../pjs-core)** - Rust implementation and core protocol
- **[PJS Server Examples](../../examples)** - Server implementations
- **[PJS Benchmarks](../pjs-bench)** - Performance comparisons

---

<div align="center">

**[Documentation](https://docs.pjs.rs) • [Examples](../../examples) • [Benchmarks](../pjs-bench) • [GitHub](https://github.com/pjs-rs/pjs)**

Made with ❤️ for the JavaScript community

</div>