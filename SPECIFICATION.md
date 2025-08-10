# Priority JSON Streaming Protocol (PJS)

Version: 1.0-draft  
Status: Draft Specification  
Authors: [Contributors]  
Date: 2024

## Abstract

Priority JSON Streaming Protocol (PJS) is a protocol for efficient, prioritized transmission of JSON documents over network streams. It enables clients to receive and process critical data immediately while less important data continues streaming in the background.

## 1. Introduction

### 1.1 Motivation

Traditional JSON APIs require complete document transmission before parsing can begin, leading to:

- High latency for time-to-first-byte (TTFB)
- Poor user experience with large payloads
- Inefficient bandwidth usage for partial data needs
- Memory overhead for large documents

PJS solves these problems by:

- Transmitting data in priority order
- Enabling incremental parsing and rendering
- Reducing perceived latency by 5-10x
- Supporting partial document transmission

### 1.2 Design Principles

1. **Progressive Enhancement** - Works with standard JSON, enhances when both sides support PJS
2. **Standards-Based** - Uses JSON Pointer (RFC 6901) or JSON Path for addressing
3. **Transport Agnostic** - Works over HTTP/1.1, HTTP/2, WebSocket, or raw TCP
4. **Backwards Compatible** - Falls back to regular JSON for non-PJS clients

### 1.3 Terminology

- **Skeleton** - Initial JSON structure with empty/default values
- **Patch** - Update to specific paths in the skeleton
- **Frame** - Single transmission unit in the protocol
- **Priority** - Numeric value (0-255) indicating transmission order
- **JSON Pointer** - Path to a value in JSON document (RFC 6901)

## 2. Protocol Overview

### 2.1 Transmission Flow

```
Client                          Server
  |                               |
  |--------- Request ------------->|
  |        (with PJS accept)       |
  |                               |
  |<-------- Skeleton ------------|
  |        (structure only)        |
  |                               |
  |<-------- Patch P100 ----------|
  |       (critical data)          |
  |                               |
  |<-------- Patch P90 -----------|
  |        (important data)        |
  |                               |
  |<-------- Patch P50 -----------|
  |        (normal data)           |
  |                               |
  |<-------- Patch P10 -----------|
  |         (low priority)         |
  |                               |
  |<-------- Complete ------------|
  |         (end signal)           |
  |                               |
```

### 2.2 Content Negotiation

Client indicates PJS support via HTTP headers:

```http
GET /api/data HTTP/1.1
Accept: application/pjs+json, application/json
PJS-Version: 1.0
PJS-Features: skeleton, patches, compression
```

Server responds with:

```http
HTTP/1.1 200 OK
Content-Type: application/pjs+json
PJS-Version: 1.0
PJS-Strategy: skeleton-first
```

## 3. Frame Format

### 3.1 Frame Structure

Each frame is a JSON object with metadata and payload:

```typescript
interface Frame {
  "@type": FrameType;           // Frame type identifier
  "@seq": number;                // Sequence number
  "@priority"?: number;          // Priority (0-255, higher = more important)
  "@timestamp"?: number;         // Unix timestamp in milliseconds
  // Frame-specific fields
}

enum FrameType {
  "skeleton",    // Initial structure
  "patch",       // Data patch
  "complete",    // Stream complete
  "error",       // Error frame
  "heartbeat"    // Keep-alive
}
```

### 3.2 Skeleton Frame

Transmits the initial document structure:

```json
{
  "@type": "skeleton",
  "@seq": 0,
  "@priority": 255,
  "@schema_version": "1.0",
  "data": {
    "user": {
      "id": null,
      "name": "",
      "profile": {
        "bio": "",
        "stats": {
          "followers": 0,
          "posts": 0
        }
      },
      "posts": []
    }
  }
}
```

### 3.3 Patch Frame

Updates specific paths in the document:

```json
{
  "@type": "patch",
  "@seq": 1,
  "@priority": 100,
  "@patches": [
    {
      "op": "replace",
      "path": "/user/id",
      "value": 12345
    },
    {
      "op": "replace",
      "path": "/user/name",
      "value": "Alice Johnson"
    }
  ]
}
```

Supported operations:

- `replace` - Replace value at path
- `add` - Add value to object or append to array
- `remove` - Remove value at path
- `move` - Move value from one path to another
- `copy` - Copy value from one path to another

### 3.4 Array Streaming

For large arrays, special handling is provided:

```json
{
  "@type": "patch",
  "@seq": 2,
  "@priority": 50,
  "@array_metadata": {
    "path": "/user/posts",
    "total_items": 1000,
    "chunk_index": 0,
    "chunk_size": 10
  },
  "@patches": [
    {
      "op": "add",
      "path": "/user/posts/-",
      "value": [
        {"id": 1, "title": "Post 1"},
        {"id": 2, "title": "Post 2"}
      ]
    }
  ]
}
```

### 3.5 Complete Frame

Signals successful stream completion:

```json
{
  "@type": "complete",
  "@seq": 99,
  "@stats": {
    "total_frames": 100,
    "total_bytes": 45678,
    "duration_ms": 234
  },
  "@checksum": "sha256:abcd1234..."
}
```

### 3.6 Error Frame

Communicates errors during streaming:

```json
{
  "@type": "error",
  "@seq": 5,
  "@error": {
    "code": "PATCH_FAILED",
    "message": "Invalid path: /user/invalid",
    "recoverable": true
  }
}
```

## 4. Priority System

### 4.1 Priority Levels

Priorities range from 0 to 255, with suggested bands:

| Priority Range | Category | Use Case |
|---------------|----------|----------|
| 200-255 | Critical | IDs, status flags, error states |
| 150-199 | High | Names, titles, key identifiers |
| 100-149 | Normal | Main content, descriptions |
| 50-99 | Low | Metadata, stats, counts |
| 0-49 | Background | Historical data, logs, archives |

### 4.2 Priority Inheritance

Nested structures inherit parent priority unless explicitly overridden:

```javascript
{
  "user": {                    // Priority: 100
    "id": 123,                // Inherits: 100
    "profile": {              // Inherits: 100
      "bio": "...",           // Inherits: 100
      "@priority": 30,        // Override for this subtree
      "interests": [...]      // Priority: 30
    }
  }
}
```

### 4.3 Dynamic Priority Adjustment

Server may adjust priorities based on:

- Network conditions (RTT, bandwidth)
- Client capabilities
- Data freshness requirements
- Business rules

## 5. Path Addressing

### 5.1 JSON Pointer (RFC 6901)

Primary addressing method:

```
/user/profile/bio           -> user.profile.bio
/posts/0/title              -> posts[0].title
/stats/total_users          -> stats.total_users
/items/-                    -> append to items array
```

### 5.2 JSON Path (Optional)

Extended addressing for complex queries:

```
$.user.posts[*].title       -> all post titles
$.user.posts[?(@.public)]   -> public posts only
$.user.posts[-1]            -> last post
```

### 5.3 Relative Paths

Within a patch batch, relative paths are supported:

```json
{
  "@type": "patch",
  "@base_path": "/user/profile",
  "@patches": [
    {"op": "replace", "path": "/bio", "value": "..."},
    {"op": "replace", "path": "/avatar", "value": "..."}
  ]
}
```

## 6. Transport Bindings

### 6.1 HTTP/1.1 with Chunked Transfer

```http
HTTP/1.1 200 OK
Content-Type: application/pjs+json
Transfer-Encoding: chunked

1a\r\n
{"@type":"skeleton"...}\n
\r\n
15\r\n
{"@type":"patch"...}\n
\r\n
0\r\n
\r\n
```

### 6.2 HTTP/2 with Server Push

Each frame can be pushed as a separate stream with priority hints.

### 6.3 WebSocket

Frames sent as individual WebSocket messages:

```javascript
ws.onmessage = (event) => {
  const frame = JSON.parse(event.data);
  processFrame(frame);
};
```

### 6.4 Server-Sent Events (SSE)

```
event: frame
data: {"@type":"skeleton","data":{...}}

event: frame
data: {"@type":"patch","@patches":[...]}

event: complete
data: {"@type":"complete"}
```

## 7. Client Implementation

### 7.1 State Management

```typescript
class PJSClient {
  private skeleton: any = null;
  private document: any = null;
  private patches: Map<number, Patch[]> = new Map();
  
  processFrame(frame: Frame): void {
    switch(frame["@type"]) {
      case "skeleton":
        this.skeleton = frame.data;
        this.document = JSON.parse(JSON.stringify(frame.data));
        this.onSkeletonReceived(this.skeleton);
        break;
        
      case "patch":
        this.applyPatches(frame["@patches"]);
        this.onPatchApplied(frame["@patches"], frame["@priority"]);
        break;
        
      case "complete":
        this.onComplete(this.document);
        break;
    }
  }
  
  private applyPatches(patches: Patch[]): void {
    for (const patch of patches) {
      applyPatch(this.document, patch);
    }
  }
}
```

### 7.2 Progressive Rendering

```javascript
client.onSkeletonReceived = (skeleton) => {
  // Render UI with loading states
  renderUIStructure(skeleton);
};

client.onPatchApplied = (patches, priority) => {
  if (priority >= 200) {
    // Critical update - render immediately
    updateUIImmediate(patches);
  } else if (priority >= 100) {
    // Normal update - batch with next frame
    requestAnimationFrame(() => updateUI(patches));
  } else {
    // Low priority - update in background
    requestIdleCallback(() => updateUI(patches));
  }
};
```

## 8. Server Implementation

### 8.1 Priority Extraction

```rust
trait PriorityExtractor {
    fn extract_priority(&self, path: &str, value: &Value) -> u8;
}

struct DefaultPriorityExtractor;

impl PriorityExtractor for DefaultPriorityExtractor {
    fn extract_priority(&self, path: &str, value: &Value) -> u8 {
        match path {
            p if p.ends_with("/id") => 250,
            p if p.ends_with("/name") || p.ends_with("/title") => 200,
            p if p.contains("/stats/") => 150,
            p if p.contains("/content") => 100,
            p if p.contains("/metadata") => 50,
            _ => 100
        }
    }
}
```

### 8.2 Streaming Pipeline

```rust
async fn stream_json<W: AsyncWrite>(
    data: Value,
    writer: &mut W,
    config: PJSConfig
) -> Result<()> {
    let skeleton = generate_skeleton(&data, config.skeleton_depth);
    let patches = extract_patches(&data, &skeleton, &config.priority_extractor);
    
    // Send skeleton
    write_frame(writer, Frame::Skeleton(skeleton)).await?;
    
    // Group and send patches by priority
    for (priority, patch_group) in group_by_priority(patches) {
        write_frame(writer, Frame::Patch {
            priority,
            patches: patch_group
        }).await?;
        
        // Optional: Flush after critical patches
        if priority >= 200 {
            writer.flush().await?;
        }
    }
    
    // Send completion
    write_frame(writer, Frame::Complete).await?;
    
    Ok(())
}
```

## 9. Performance Considerations

### 9.1 Benchmarks

Expected performance improvements:

| Metric | Traditional JSON | PJS | Improvement |
|--------|-----------------|-----|-------------|
| Time to First Byte | 0ms | 0ms | Same |
| Time to First Render | 500ms | 50ms | 10x |
| Time to Interactive | 1000ms | 200ms | 5x |
| Memory Usage (10MB JSON) | 30MB | 10MB | 3x |
| CPU Usage | Baseline | 80% | 1.25x |

### 9.2 Optimization Strategies

1. **Batch small patches** to reduce frame overhead
2. **Compress paths** using dictionary encoding for repeated paths
3. **Use binary framing** (MessagePack, CBOR) for further size reduction
4. **Implement path prediction** based on access patterns
5. **Enable HTTP/2 multiplexing** for parallel patch streams

## 10. Security Considerations

### 10.1 Path Validation

Servers MUST validate all paths to prevent:

- Path traversal attacks
- Infinite loops in circular references
- Memory exhaustion from deep nesting

### 10.2 Size Limits

Recommended limits:

- Maximum frame size: 1MB
- Maximum path depth: 100
- Maximum array size per frame: 10,000 items
- Maximum total patches: 100,000

### 10.3 Authentication

PJS frames should be authenticated using standard transport security:

- HTTPS for HTTP transport
- WSS for WebSocket transport
- Include authentication tokens in initial handshake

## 11. Examples

### 11.1 E-commerce Product Listing

```javascript
// Request
GET /api/products?category=electronics
Accept: application/pjs+json
PJS-Priority-Hint: price,title,thumbnail

// Response frames
// Frame 1: Skeleton
{
  "@type": "skeleton",
  "data": {
    "products": [],
    "total": 0,
    "filters": {}
  }
}

// Frame 2: Critical data (IDs and titles)
{
  "@type": "patch",
  "@priority": 200,
  "@patches": [
    {"op": "replace", "path": "/total", "value": 1247},
    {"op": "add", "path": "/products/-", "value": [
      {"id": 1, "title": "iPhone 15", "price": null, "image": null},
      {"id": 2, "title": "Samsung S24", "price": null, "image": null}
    ]}
  ]
}

// Frame 3: Prices (high priority for e-commerce)
{
  "@type": "patch",
  "@priority": 180,
  "@patches": [
    {"op": "replace", "path": "/products/0/price", "value": 999},
    {"op": "replace", "path": "/products/1/price", "value": 899}
  ]
}

// Frame 4: Images (lower priority)
{
  "@type": "patch",
  "@priority": 100,
  "@patches": [
    {"op": "replace", "path": "/products/0/image", "value": "https://..."},
    {"op": "replace", "path": "/products/1/image", "value": "https://..."}
  ]
}
```

### 11.2 Real-time Dashboard

```javascript
// Initial connection
ws = new WebSocket("wss://api.example.com/dashboard");
ws.send(JSON.stringify({
  "type": "subscribe",
  "accept": "application/pjs+json",
  "priorities": {
    "metrics.errors": 255,
    "metrics.requests": 200,
    "metrics.latency": 150,
    "logs": 50
  }
}));

// Continuous streaming
ws.onmessage = (event) => {
  const frame = JSON.parse(event.data);
  
  if (frame["@priority"] >= 200) {
    // Update critical metrics immediately
    updateCriticalMetrics(frame["@patches"]);
  } else {
    // Buffer and batch lower priority updates
    bufferUpdate(frame);
  }
};
```

## 12. Reference Implementation

Reference implementations are available at:

- Rust: [github.com/example/pjs-rust](https://github.com)
- JavaScript: [github.com/example/pjs-js](https://github.com)
- Go: [github.com/example/pjs-go](https://github.com)

## 13. Appendices

### Appendix A: MIME Type Registration

```
Type name: application
Subtype name: pjs+json
Required parameters: none
Optional parameters: 
  version: Protocol version (default "1.0")
  strategy: Streaming strategy (skeleton-first, progressive, delta)
```

### Appendix B: Related Standards

- RFC 6901 - JSON Pointer
- RFC 6902 - JSON Patch
- RFC 7159 - JSON Data Interchange Format
- RFC 9535 - JSON Path
- RFC 7540 - HTTP/2
- W3C Server-Sent Events

### Appendix C: Changelog

- v1.0-draft (2025-05): Initial draft specification