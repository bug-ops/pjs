# PJS WASM - WebAssembly Bindings for Priority JSON Streaming

WebAssembly bindings for the PJS (Priority JSON Streaming) protocol, enabling high-performance JSON parsing and streaming in web browsers and Node.js environments.

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

- Rust nightly toolchain (required by parent project)
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

Note: This WASM build uses `serde_json` instead of `sonic-rs` (used in native builds) because `sonic-rs` is not WASM-compatible. While slightly slower, `serde_json` provides excellent performance and full WASM support.

## Architecture

`pjs-wasm` is built on top of `pjs-domain`, a pure Rust domain layer with:
- Zero external I/O dependencies
- WASM-compatible error types
- Clean architecture principles
- Full test coverage

```
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

## Future Enhancements

Planned features for future versions:

- [ ] Progressive streaming support
- [ ] Priority-based partial parsing
- [ ] Schema validation API
- [ ] Custom priority configuration
- [ ] WebSocket streaming integration
- [ ] Compression support
- [ ] Worker thread support for heavy parsing
