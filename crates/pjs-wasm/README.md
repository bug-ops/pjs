# PJS WASM - WebAssembly Bindings for Priority JSON Streaming

[![npm](https://img.shields.io/npm/v/pjs-wasm)](https://www.npmjs.com/package/pjs-wasm)
[![Bundle Size](https://img.shields.io/bundlephobia/minzip/pjs-wasm)](https://bundlephobia.com/package/pjs-wasm)
[![License](https://img.shields.io/crates/l/pjson-rs)](../../LICENSE-MIT)

WebAssembly bindings for the PJS (Priority JSON Streaming) protocol, enabling high-performance JSON parsing and streaming in web browsers and Node.js environments.

> [!NOTE]
> This package is part of the [PJS workspace](https://github.com/bug-ops/pjs). For Rust usage, see the main `pjson-rs` crate.

## Overview

`pjs-wasm` provides a JavaScript-friendly interface to the core PJS domain logic. It's compiled to WebAssembly for optimal performance while maintaining compatibility with web standards.

## Features

- **Zero-copy JSON parsing** where possible
- **Priority-based streaming** support
- **Schema validation** capabilities
- **Optimized for size** - minimal WASM bundle using `opt-level = "s"`
- **Full TypeScript support** - TypeScript definitions generated automatically

## Installation

### Using npm

```bash
npm install pjs-wasm
```

### Using wasm-pack (for development)

```bash
# Build for web (bundler target)
wasm-pack build --target web

# Build for Node.js
wasm-pack build --target nodejs

# Build for bundler (webpack, rollup, etc.)
wasm-pack build --target bundler
```

## Usage

### Basic Parsing

```javascript
import { PjsParser, version } from 'pjs-wasm';

// Check version
console.log(`Using PJS WASM version: ${version()}`);

// Create parser instance
const parser = new PjsParser();

// Parse JSON
const jsonString = '{"name": "Alice", "age": 30, "city": "NYC"}';
try {
    const result = parser.parse(jsonString);
    console.log(result);
} catch (error) {
    console.error('Parse error:', error);
}
```

### In Browser (ES Modules)

```html
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>PJS WASM Demo</title>
</head>
<body>
    <script type="module">
        import init, { PjsParser, version } from './pkg/pjs_wasm.js';

        async function run() {
            // Initialize WASM module
            await init();

            console.log(`PJS WASM ${version()}`);

            const parser = new PjsParser();
            const data = parser.parse('{"message": "Hello from WASM!"}');
            console.log(data);
        }

        run();
    </script>
</body>
</html>
```

### With TypeScript

```typescript
import { PjsParser, version } from 'pjs-wasm';

const parser: PjsParser = new PjsParser();

try {
    const result = parser.parse('{"key": "value"}');
    console.log(result);
} catch (error) {
    console.error('Failed to parse:', error);
}
```

## API Reference

### `PjsParser`

Main parser class for JSON parsing.

#### Constructor

```javascript
const parser = new PjsParser();
```

#### Methods

##### `parse(jsonStr: string): any`

Parse a JSON string and return the parsed object.

**Parameters:**

- `jsonStr` - The JSON string to parse

**Returns:**

- Parsed JSON value

**Throws:**

- Error if the JSON is invalid

**Example:**

```javascript
const parser = new PjsParser();
const data = parser.parse('{"id": 1, "name": "test"}');
```

##### `version(): string` (static)

Get the parser version.

**Returns:**

- Version string

**Example:**

```javascript
const v = PjsParser.version();
console.log(v); // "0.1.0"
```

##### `generateFrames(jsonStr: string, minPriority: number): Frame[]`

Generate priority-based frames from JSON data.

**Parameters:**

- `jsonStr` - The JSON string to convert to frames
- `minPriority` - Minimum priority threshold (1-255)

**Returns:**

- Array of frames ordered by priority (highest first)

**Throws:**

- Error if JSON is invalid or priority is out of range

**Example:**

```javascript
const parser = new PjsParser();
const frames = parser.generateFrames('{"id": 1, "name": "Alice"}', 50);
// Returns: [skeleton, critical_patches, high_patches, complete]
```

##### `withConfig(config: PriorityConfigBuilder): PjsParser` (static)

Create a parser with custom priority configuration.

**Parameters:**

- `config` - Priority configuration builder

**Returns:**

- New parser instance with custom configuration

**Example:**

```javascript
const config = new PriorityConfigBuilder()
    .addCriticalField('user_id');
const parser = PjsParser.withConfig(config);
```

### `version(): string`

Get the WASM module version.

**Returns:**

- Version string

**Example:**

```javascript
import { version } from 'pjs-wasm';
console.log(version()); // "0.1.0"
```

## Building from Source

### Prerequisites

> [!WARNING]
> Building from source requires **nightly Rust** for GAT features.

- Rust nightly toolchain: `rustup install nightly`
- wasm-pack: `cargo install wasm-pack`

### Build Commands

```bash
# Development build
wasm-pack build

# Release build (optimized for size)
wasm-pack build --release

# Build for specific target
wasm-pack build --target web --release
wasm-pack build --target nodejs --release
wasm-pack build --target bundler --release
```

### Testing

```bash
# Run Rust tests
cargo test

# Run WASM tests in browser
wasm-pack test --chrome
wasm-pack test --firefox

# Run WASM tests in Node.js
wasm-pack test --node
```

## Performance Considerations

### Bundle Size

The WASM binary is optimized for size using:

- `opt-level = "s"` - Optimize for small binary size
- `lto = true` - Link-time optimization
- `wasm-opt = ["-Os"]` - Additional post-processing optimization

Typical bundle sizes:

- **Uncompressed**: ~100-200 KB
- **Gzip compressed**: ~40-80 KB
- **Brotli compressed**: ~35-70 KB

### Parser Selection

> [!NOTE]
> This WASM build uses `serde_json` instead of `sonic-rs` (used in native builds) because `sonic-rs` is not WASM-compatible. While slightly slower, `serde_json` provides excellent performance and full WASM support.

## Architecture

`pjs-wasm` is built on top of `pjs-domain`, a pure Rust domain layer with:

- Zero external I/O dependencies
- WASM-compatible error types
- Clean architecture principles
- Full test coverage

```text
pjs-wasm (WebAssembly bindings)
    ↓
pjs-domain (Pure domain logic)
    ↓
Value objects, entities, services
```

## Browser Compatibility

Requires browsers with WebAssembly support:

- Chrome/Edge 57+
- Firefox 52+
- Safari 11+
- Node.js 8+

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT license ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Contributing

Contributions are welcome! Please read the [Contributing Guide](../../CONTRIBUTING.md) first.

## Links

- [PJS GitHub Repository](https://github.com/bug-ops/pjs)
- [PJS Specification](../../SPECIFICATION.md)
- [API Documentation](https://docs.rs/pjson-rs)
- [wasm-bindgen Documentation](https://rustwasm.github.io/docs/wasm-bindgen/)

## Advanced Features

### Priority-Based Frame Generation

Generate priority-ordered frames from JSON data:

```javascript
import { PjsParser, PriorityConstants } from 'pjs-wasm';

const parser = new PjsParser();

// Generate frames ordered by priority
const frames = parser.generateFrames(
    JSON.stringify({
        id: 1,
        name: "Alice",
        bio: "Software developer",
        analytics: { views: 1000 }
    }),
    PriorityConstants.MEDIUM  // Minimum priority threshold
);

// Frames are ordered: skeleton → critical → high → medium → complete
```

### Priority Constants

```javascript
import { PriorityConstants } from 'pjs-wasm';

PriorityConstants.CRITICAL    // 100 - Essential data (IDs, status)
PriorityConstants.HIGH        // 80  - Important visible data (names, titles)
PriorityConstants.MEDIUM      // 50  - Regular content
PriorityConstants.LOW         // 25  - Supplementary data
PriorityConstants.BACKGROUND  // 10  - Analytics, logs
```

### Custom Priority Configuration

Customize which fields receive which priorities:

```javascript
import { PjsParser, PriorityConfigBuilder } from 'pjs-wasm';

const config = new PriorityConfigBuilder()
    .addCriticalField('user_id')
    .addCriticalField('session_id')
    .addHighField('display_name')
    .addHighField('email')
    .addLowPattern('debug')           // Fields containing "debug" → low priority
    .addBackgroundPattern('trace')    // Fields containing "trace" → background priority
    .setLargeArrayThreshold(200)      // Arrays >200 elements → background priority
    .setLargeStringThreshold(5000);   // Strings >5000 chars → low priority

const parser = PjsParser.withConfig(config);

const frames = parser.generateFrames(
    JSON.stringify({ user_id: 123, display_name: "Alice" }),
    PriorityConstants.LOW
);
```

### Progressive Rendering Example

```javascript
const parser = new PjsParser();
const frames = parser.generateFrames(jsonData, PriorityConstants.MEDIUM);

for (const frame of frames) {
    switch (frame.frame_type) {
        case 'Skeleton':
            renderLoadingSkeleton(frame.payload);
            break;
        case 'Patch':
            applyDataPatches(frame.payload.patches);
            break;
        case 'Complete':
            finalizeRendering();
            break;
    }
}
```

## Future Enhancements

Planned features for future versions:

- [x] Progressive streaming support
- [x] Priority-based partial parsing
- [ ] Schema validation API
- [x] Custom priority configuration
- [ ] WebSocket streaming integration
- [ ] Compression support
- [ ] Worker thread support for heavy parsing
