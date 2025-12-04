# PJS WASM Browser Demo

Interactive demonstration of the PJS (Priority JSON Streaming Protocol) WebAssembly implementation with advanced features.

## Features

### 1. Transport Switcher
Switch between two transport modes:
- **WASM (Local)**: Direct WebAssembly processing with zero network latency
- **HTTP Mock**: Simulated HTTP transport with network delay for comparison

Use this to see the performance difference between local WASM processing and traditional client-server JSON streaming.

### 2. Performance Comparison Widget
Run benchmarks to compare:
- **PJS WASM** vs **Traditional JSON.parse()**
- Shows average, min, and max times over 100 iterations
- Calculates speedup multiplier and percentage improvement

Click "Run Benchmark" to see real performance metrics.

### 3. Real-time Metrics Display
Monitor streaming performance in real-time:
- **Memory Usage**: JavaScript heap size (if available via `performance.memory`)
- **Throughput**: Frames processed per second
- **Time to First Frame**: Latency before first data arrives
- **Progress**: Visual progress bar with percentage

### 4. Sample Data Presets
Choose from pre-configured JSON samples:
- **Small (1KB)**: User profile with basic fields
- **Medium (10KB)**: Product catalog with 50 items
- **Large (100KB)**: Analytics data with daily metrics

Or use "Custom" to enter your own JSON.

### 5. UI/UX Enhancements
- **Mobile-responsive design**: Works on phones, tablets, and desktops
- **Keyboard shortcuts**:
  - `Ctrl+Enter`: Start streaming
  - `Escape`: Clear output
- **Better error visualization**: Animated error messages with clear formatting
- **Loading states**: Spinner and status indicators during processing
- **Input size display**: Shows JSON size in bytes/KB/MB

## Running the Demo

### Prerequisites

1. Build the WASM package:
   ```bash
   cd crates/pjs-wasm
   wasm-pack build --target web --release
   ```

2. Serve the demo directory with an HTTP server (required for ES modules):
   ```bash
   # Using Python 3
   cd demo
   python3 -m http.server 8000

   # Or using Node.js
   npx http-server -p 8000

   # Or using Rust
   cargo install simple-http-server
   simple-http-server -p 8000
   ```

3. Open in browser:
   ```
   http://localhost:8000
   ```

## File Structure

```
demo/
├── index.html      # Main HTML structure
├── styles.css      # All styling (mobile-responsive)
├── app.js          # JavaScript application logic
└── README.md       # This file
```

## Architecture

### HTML Structure
- **Header**: Title, subtitle, transport switcher
- **Performance Panel**: Benchmark runner and results
- **Metrics Panel**: Real-time metrics and progress bar
- **Demo Grid**: Input/output panels (side-by-side on desktop, stacked on mobile)
- **Stats Panel**: Summary statistics (frames, bytes, time)

### JavaScript (app.js)
- **PJSDemo class**: Main application controller
- **Transport abstraction**: Switches between WASM and HTTP Mock
- **Metrics tracking**: Collects performance data
- **Benchmark runner**: Automated performance comparison
- **Event handling**: UI interactions and keyboard shortcuts

### CSS (styles.css)
- **CSS Variables**: Centralized theming (colors, spacing)
- **Mobile-first**: Responsive grid layouts
- **Animations**: Smooth transitions and slide-in effects
- **Accessibility**: Clear focus states, readable contrast

## Browser Compatibility

Requires modern browsers with:
- WebAssembly support
- ES6 modules support
- Performance API (optional for memory metrics)

**Tested on:**
- Chrome/Edge 90+
- Firefox 88+
- Safari 14+

## Performance Notes

### WASM vs Traditional Parsing

The benchmark compares:
1. **PJS WASM**: Full priority-based streaming with frame generation
2. **JSON.parse()**: Standard JavaScript parsing

**Expected results:**
- WASM may be slightly slower for small payloads (overhead)
- WASM shows advantages with larger datasets (10KB+)
- Priority streaming provides faster time-to-first-frame

### Memory Usage

If your browser supports `performance.memory`:
- Memory usage is displayed in MB
- Useful for tracking memory efficiency
- Note: Only available in Chrome with `--enable-precise-memory-info` flag

## Customization

### Adding New Presets

Edit `PRESETS` in `app.js`:

```javascript
const PRESETS = {
    mypreset: {
        name: 'My Custom Data',
        size: '~5KB',
        data: {
            // Your JSON structure
        }
    }
};
```

### Adjusting Network Delay

Change `networkDelay` in `streamWithHTTPMock()`:

```javascript
const networkDelay = 50; // ms per frame
```

### Custom Priority Configuration

Use `PriorityConfigBuilder` in WASM:

```javascript
const config = new PriorityConfigBuilder()
    .addCriticalField('id')
    .addHighField('name');
const stream = PriorityStream.withConfig(config);
```

## Troubleshooting

### WASM Module Not Found
Ensure you built the package:
```bash
wasm-pack build --target web --release
```

The build creates `pkg/pjs_wasm.js` and `pkg/pjs_wasm_bg.wasm`.

### ES Module Errors
Ensure you're serving via HTTP, not `file://`:
```bash
python3 -m http.server 8000
```

### CORS Errors
If loading from a different domain, ensure proper CORS headers.

### Performance Memory API Unavailable
`performance.memory` is Chrome-specific. Run Chrome with:
```bash
chrome --enable-precise-memory-info
```

Or memory will show "N/A" in other browsers (this is expected).

## Future Enhancements

Potential additions:
- [ ] Dark/light theme toggle
- [ ] Export benchmark results (CSV/JSON)
- [ ] WebSocket streaming support
- [ ] Schema validation visualization
- [ ] Compression comparison
- [ ] Frame replay/debugging tools
- [ ] Custom priority configuration UI

## License

Same as parent project (MIT OR Apache-2.0).
