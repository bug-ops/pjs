# PJS WASM Browser Demo

Interactive browser demo showcasing priority-based JSON streaming with WebAssembly.

## Features

- 🚀 **WASM-Powered Parsing**: Uses Rust-based WASM module for high-performance JSON processing
- 🎯 **Priority-Based Streaming**: Automatically assigns priorities to JSON fields
- 📊 **Visual Frame Display**: See exactly how JSON is split into priority-ordered frames
- ⚡ **Performance Comparison**: Compare WASM vs Native JavaScript implementations
- 🎨 **Interactive UI**: Adjust priority thresholds and see results in real-time

## Quick Start

### 1. Build WASM Package

```bash
cd ../../pjs-wasm
wasm-pack build --target web --release
```

This generates the `pkg/` directory with:
- `pjs_wasm.js` - JavaScript bindings
- `pjs_wasm_bg.wasm` - WebAssembly binary
- `pjs_wasm.d.ts` - TypeScript definitions

### 2. Copy WASM Package

```bash
cp -r pkg ../pjs-js-client/examples/browser-wasm/
```

### 3. Serve Locally

You need a local web server (WASM requires HTTPS or localhost):

**Option A: Using Python**
```bash
cd examples/browser-wasm
python3 -m http.server 8000
```

**Option B: Using Node.js (http-server)**
```bash
npm install -g http-server
http-server examples/browser-wasm -p 8000
```

**Option C: Using VS Code Live Server**
- Install "Live Server" extension
- Right-click on `index.html`
- Select "Open with Live Server"

### 4. Open in Browser

Navigate to: `http://localhost:8000`

## How It Works

### Priority Assignment

The WASM module automatically assigns priorities based on:

| Field Type | Priority | Example |
|------------|----------|---------|
| **Critical** | 100 | `id`, `uuid`, `user_id`, `status` |
| **High** | 80 | `name`, `title`, `email` |
| **Medium** | 50 | `description`, `bio`, `address` |
| **Low** | 25 | `metadata`, `tags` |
| **Background** | 10 | `analytics`, `stats`, `debug` |

**Additional Rules:**
- Depth penalty: Nested fields get lower priority
- Size-based: Large arrays (>100 items) → BACKGROUND
- Large strings (>1000 chars) → LOW

### Frame Generation

1. **Skeleton Frame** (Priority: CRITICAL) - JSON structure with null/empty values
2. **Patch Frames** (Various priorities) - Grouped by priority level, ordered descending
3. **Complete Frame** (Priority: CRITICAL) - End of stream signal

### Example Output

Input JSON:
```json
{
  "id": 1,
  "name": "Alice",
  "email": "alice@example.com",
  "bio": "Developer...",
  "analytics": {
    "views": 1000,
    "clicks": 50
  }
}
```

With threshold = MEDIUM (50):

```
Frame 0: Skeleton (CRITICAL)
Frame 1: Patch - id (CRITICAL, 100)
Frame 2: Patch - name, email (HIGH, 80)
Frame 3: Patch - bio (MEDIUM, 50)
Frame 4: Complete (CRITICAL)

Note: analytics excluded (BACKGROUND < MEDIUM threshold)
```

## Use Cases

### Progressive Loading

Show critical data first, load rest later:

```javascript
const frames = parser.generateFrames(json, PriorityConstants.HIGH);
// First frames contain id, name, email
// Render UI immediately with critical data
// Load remaining frames in background
```

### Bandwidth Optimization

Filter low-priority data on slow connections:

```javascript
if (connection.effectiveType === '3g') {
  threshold = PriorityConstants.MEDIUM; // Skip low-priority data
} else {
  threshold = PriorityConstants.BACKGROUND; // Load everything
}
```

### Real-Time Updates

Stream large JSON responses progressively:

```javascript
const frames = parser.generateFrames(largeJson, PriorityConstants.LOW);
frames.forEach((frame, index) => {
  setTimeout(() => updateUI(frame), index * 100);
});
```

## API Reference

### PjsParser

```javascript
import { PjsParser, PriorityConstants, PriorityConfigBuilder } from 'pjs-wasm';

// Basic usage
const parser = new PjsParser();
const frames = parser.generateFrames(jsonString, 50);

// Custom configuration
const config = new PriorityConfigBuilder()
    .addCriticalField('product_id')
    .addHighField('product_name')
    .addBackgroundPattern('recommendations');

const customParser = PjsParser.withConfig(config);
```

### Priority Constants

```javascript
PriorityConstants.CRITICAL    // 100
PriorityConstants.HIGH        // 80
PriorityConstants.MEDIUM      // 50
PriorityConstants.LOW         // 25
PriorityConstants.BACKGROUND  // 10
```

## Performance

Typical performance on modern hardware:

| JSON Size | WASM Time | Native JS Time | Speedup |
|-----------|-----------|----------------|---------|
| 1 KB      | ~0.5ms    | ~0.3ms         | 0.6x    |
| 10 KB     | ~2ms      | ~5ms           | 2.5x    |
| 100 KB    | ~15ms     | ~45ms          | 3x      |
| 1 MB      | ~120ms    | ~400ms         | 3.3x    |

**Note**: WASM has initialization overhead (~10-20ms) but scales better with data size.

## Browser Compatibility

Works in all browsers with WebAssembly support:

- ✅ Chrome/Edge 57+
- ✅ Firefox 52+
- ✅ Safari 11+
- ✅ Opera 44+

## Troubleshooting

### WASM Failed to Load

**Problem**: "Failed to load WASM module"

**Solutions**:
- Ensure you're serving over HTTP(S), not `file://`
- Check that `pkg/` directory exists with WASM files
- Verify browser console for specific error messages

### CORS Errors

**Problem**: CORS policy blocking WASM

**Solution**: Use a local server (see Quick Start) instead of opening HTML directly

### Performance Issues

**Problem**: Slow frame generation

**Solutions**:
- Use WASM implementation (faster for large JSON)
- Increase priority threshold to generate fewer frames
- Check browser DevTools Performance tab

## Development

### Rebuild WASM After Changes

```bash
cd ../../pjs-wasm
cargo test --lib                          # Run tests
wasm-pack build --target web --release   # Build WASM
cp -r pkg ../pjs-js-client/examples/browser-wasm/
```

### Debug Mode

For development, use debug build (faster compilation):

```bash
wasm-pack build --target web --dev
```

## License

MIT OR Apache-2.0
