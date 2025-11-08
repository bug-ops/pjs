# PJS WebAssembly Implementation Roadmap

**Status**: Planning Phase
**Version**: 0.1.0-draft
**Last Updated**: 2025-11-08

## Executive Summary

This document outlines the strategy for creating a WebAssembly (WASM) module for PJS (Priority JSON Streaming Protocol) to enable client-side usage in JavaScript/TypeScript applications. The implementation will extract core domain logic into a WASM-compatible crate while maintaining the high-performance native implementation.

**Key Decision**: Separate `pjs-domain` and `pjs-wasm` crates to avoid polluting the native codebase with conditional compilation.

**Testing Strategy**: WASM implementation will be validated through integration with the existing `pjs-js-client` crate.

## Goals

### Primary Goals

- ✅ Enable PJS usage in web browsers via WASM
- ✅ Provide TypeScript bindings for seamless JS/TS integration
- ✅ Integrate with existing pjs-js-client for real-world validation
- ✅ Maintain Clean Architecture principles
- ✅ Keep performance acceptable (understanding WASM trade-offs)

### Non-Goals

- ❌ Match native performance (WASM will be ~5x slower)
- ❌ Support all features (focus on core parsing and validation)
- ❌ Replace native implementation

## Architecture Strategy

### Workspace Structure

```
crates/
├── pjs-core/           # Existing native implementation (unchanged)
├── pjs-domain/         # NEW: Pure domain logic (WASM-compatible)
│   ├── value_objects/  # Priority, JsonPath, JsonData, Schema
│   ├── entities/       # Stream, Frame
│   ├── events/         # Domain events
│   └── services/       # ValidationService
├── pjs-wasm/           # NEW: WASM bindings
│   ├── src/
│   │   ├── lib.rs      # wasm-bindgen exports
│   │   ├── parser.rs   # JSON parsing wrapper
│   │   ├── api.rs      # JS/TS friendly API
│   │   └── types.rs    # Type conversions
│   └── pkg/            # Generated npm package
├── pjs-js-client/      # Existing JS/TS client (testing ground)
│   ├── src/
│   │   └── wasm/       # NEW: WASM integration layer
│   └── tests/
│       └── wasm.test.ts # NEW: WASM integration tests
├── pjs-bench/          # Existing benchmarks
└── pjs-demo/           # Existing demos
```

### WASM Compatibility Matrix

| Component | WASM Compatible | Status | Solution |
|-----------|----------------|--------|----------|
| **Domain Value Objects** | ✅ Yes | Ready | Pure Rust, no deps |
| **Domain Entities** | ✅ Yes | Ready | Simple structs |
| **Schema Validation** | ✅ Yes | Ready | Pure logic |
| **Basic Parsing** | ✅ Yes | Use serde_json | Fallback from sonic-rs |
| **sonic-rs SIMD** | ❌ No | Incompatible | Replace with serde_json |
| **Tokio Runtime** | ❌ No | Incompatible | Use wasm-bindgen-futures |
| **DashMap** | ⚠️ Unknown | Test needed | May need replacement |
| **Axum/HTTP Server** | ❌ No | Not needed | Client-side only |

## Implementation Phases

### Phase 1: Extract Domain Layer (Week 1)

**Goal**: Create `pjs-domain` crate with pure business logic

**Tasks**:
1. Create `crates/pjs-domain/` directory structure
2. Copy domain modules from `pjs-core/src/domain/`:
   - `value_objects/priority.rs`
   - `value_objects/json_path.rs`
   - `value_objects/json_data.rs`
   - `value_objects/schema.rs`
   - `entities/frame.rs`
   - `entities/stream.rs`
   - `services/validation_service.rs`
3. Remove all external dependencies (tokio, sonic-rs, etc.)
4. Add `no_std` support with `alloc` feature
5. Ensure all tests pass in isolation

**Deliverables**:
- `crates/pjs-domain/Cargo.toml`
- `crates/pjs-domain/src/lib.rs`
- Working tests: `cargo test -p pjs-domain`

**Success Criteria**:
- Zero external dependencies in domain layer
- Compiles with `--no-default-features`
- All domain tests passing

### Phase 2: Create WASM Crate (Week 2)

**Goal**: Set up `pjs-wasm` with basic structure

**Tasks**:
1. Create `crates/pjs-wasm/` directory
2. Configure `Cargo.toml` with:
   - `crate-type = ["cdylib", "rlib"]`
   - `wasm-bindgen` dependencies
   - `serde-wasm-bindgen` for JS interop
3. Implement basic exports:
   - `PjsParser` wrapper class
   - `parse()` method
   - Simple frame generation
4. Generate TypeScript bindings
5. Test WASM build: `wasm-pack build --target web`

**Deliverables**:
- `crates/pjs-wasm/Cargo.toml`
- `crates/pjs-wasm/src/lib.rs`
- Generated `pkg/` with npm package
- TypeScript type definitions

**Success Criteria**:
- `wasm-pack build` completes successfully
- Bundle size < 200 KB (gzipped)
- TypeScript types generated

### Phase 3: Implement Core Parsing (Week 3)

**Goal**: Working JSON parsing in browser

**Tasks**:
1. Implement `PjsParser::parse()`:
   - Use `serde_json` for parsing
   - Convert to `JsonData`
   - Generate frames with priorities
2. Add error handling with JS-friendly errors
3. Implement frame iterator API
4. Add `wasm-bindgen-test` unit tests

**Deliverables**:
- Working parser in WASM
- Error handling with `Result<JsValue, JsValue>`
- Browser-runnable tests

**Success Criteria**:
- Parse 100KB JSON in < 10ms
- Proper error messages in JS console
- All WASM tests passing

### Phase 4: Integration with pjs-js-client (Week 4)

**Goal**: Validate WASM implementation through existing JS client

**Tasks**:
1. Add WASM backend to `pjs-js-client`:
   - Create `src/wasm/` directory
   - Implement WASM transport adapter
   - Add feature detection (WASM vs HTTP)
2. Update `pjs-js-client` tests:
   - Add WASM-specific test suite
   - Test WASM parser integration
   - Compare WASM vs native performance
3. Create dual-mode examples:
   - Show WASM for browser
   - Show HTTP for Node.js
4. Update documentation

**Deliverables**:
- `pjs-js-client/src/wasm/wasm-backend.ts`
- `pjs-js-client/tests/wasm.test.ts`
- Updated README with WASM usage
- Performance comparison benchmarks

**Success Criteria**:
- All existing pjs-js-client tests pass with WASM backend
- Feature detection works correctly
- Documentation explains when to use WASM vs HTTP

### Phase 5: Browser Demo (Week 5)

**Goal**: Interactive browser demonstration

**Tasks**:
1. Create `examples/wasm-demo/` directory
2. Build HTML interface:
   - JSON input textarea
   - Parse button
   - Results display with priority visualization
3. Use pjs-js-client with WASM backend
4. Add error handling and logging
5. Style with basic CSS

**Deliverables**:
- `examples/wasm-demo/index.html`
- Working browser demo using pjs-js-client
- README with instructions

**Success Criteria**:
- Demo works in Chrome, Firefox, Safari
- Visual feedback for priority levels
- Error handling works correctly
- Shows real-world pjs-js-client usage

### Phase 6: Advanced Features (Week 6-7)

**Goal**: Production-ready WASM module

**Tasks**:
1. Implement streaming API (`PriorityStream`)
2. Add schema validation support
3. WebSocket client (via `web-sys`)
4. Performance optimizations
5. Comprehensive documentation

**Deliverables**:
- Streaming API
- Schema validation
- WebSocket integration
- Performance benchmarks

**Success Criteria**:
- Progressive streaming works
- Schema validation functional
- WebSocket demo working

### Phase 7: Publishing & CI/CD (Week 8)

**Goal**: Publish to npm and automate builds

**Tasks**:
1. Configure npm package metadata
2. Add GitHub Actions workflow for WASM
3. Publish to npm as `@pjs/wasm`
4. Update pjs-js-client to use published package
5. Create migration guide

**Deliverables**:
- npm package published
- CI/CD pipeline working
- Complete documentation
- Updated pjs-js-client with WASM support

**Success Criteria**:
- `npm install @pjs/wasm` works
- pjs-js-client correctly imports WASM module
- CI builds WASM on every commit
- Documentation complete

## WASM Integration with pjs-js-client

### Integration Architecture

The WASM module will be integrated into pjs-js-client as an alternative backend:

```typescript
// pjs-js-client/src/wasm/wasm-backend.ts
import init, { PjsParser } from '@pjs/wasm';

export class WasmBackend implements ParserBackend {
  private parser: PjsParser | null = null;

  async initialize(): Promise<void> {
    await init(); // Initialize WASM module
    this.parser = new PjsParser();
  }

  parse(json: string): ParseResult {
    if (!this.parser) {
      throw new Error('WASM backend not initialized');
    }
    return this.parser.parse(json);
  }
}

// Auto-detect environment and choose backend
export function createOptimalBackend(): ParserBackend {
  if (typeof WebAssembly !== 'undefined') {
    return new WasmBackend();
  } else {
    return new HttpBackend();
  }
}
```

### Testing Strategy in pjs-js-client

```typescript
// pjs-js-client/tests/wasm.test.ts
import { WasmBackend } from '../src/wasm/wasm-backend';

describe('WASM Backend', () => {
  let backend: WasmBackend;

  beforeAll(async () => {
    backend = new WasmBackend();
    await backend.initialize();
  });

  test('parses simple JSON', () => {
    const result = backend.parse('{"name": "test"}');
    expect(result.frames).toBeDefined();
  });

  test('handles priority-based streaming', async () => {
    const stream = backend.createStream(largeJson);
    const frames = [];

    while (stream.hasMore()) {
      frames.push(stream.nextFrame());
    }

    // Verify priority order
    expect(frames[0].priority).toBeGreaterThan(frames[1].priority);
  });

  test('validates against schema', () => {
    const schema = { type: 'object', required: ['id'] };
    const result = backend.validate({ id: 123 }, schema);
    expect(result.valid).toBe(true);
  });
});
```

## Technical Decisions

### Decision 1: Separate Crates vs Feature Flags

**Chosen**: Separate `pjs-domain` and `pjs-wasm` crates

**Rationale**:
- ✅ No conditional compilation in pjs-core
- ✅ Clear separation of concerns
- ✅ Independent versioning
- ❌ Some code duplication (acceptable)

### Decision 2: Parser Implementation

**Chosen**: Use `serde_json` for WASM builds

**Rationale**:
- ✅ WASM-compatible (proven)
- ✅ Well-tested and stable
- ❌ ~5x slower than sonic-rs
- Note: May explore WASM SIMD in future

### Decision 3: Async Handling

**Chosen**: Use `wasm-bindgen-futures` for browser async

**Rationale**:
- ✅ Browser event loop integration
- ✅ Standard solution for WASM
- ❌ Cannot use Tokio in WASM

### Decision 4: Bundle Size Optimization

**Chosen**: `opt-level = "s"` (optimize for size)

**Rationale**:
- ✅ Smaller download (critical for web)
- ❌ Slightly slower than `opt-level = 3`
- Acceptable trade-off for web usage

### Decision 5: Integration Testing

**Chosen**: Use pjs-js-client as primary testing ground

**Rationale**:
- ✅ Real-world usage validation
- ✅ Existing test infrastructure
- ✅ Ensures WASM API matches JS expectations
- ✅ Validates dual-mode (WASM/HTTP) operation

## Performance Expectations

### Realistic Performance Targets

| Metric | Native (sonic-rs) | WASM (serde_json) | Delta |
|--------|-------------------|-------------------|-------|
| Parse 10KB JSON | ~150 µs | ~800 µs | **5.3x slower** |
| Parse 1MB JSON | ~12 ms | ~65 ms | **5.4x slower** |
| Memory overhead | 2x input | 4x input | **2x more** |
| Bundle size | N/A | ~150 KB (gzipped) | - |

**Conclusion**: WASM will be significantly slower but acceptable for client-side use cases.

### Mitigation Strategies

- Use Web Workers for large JSON parsing
- Implement progressive streaming to prevent UI blocking
- Cache parsed results when possible
- Investigate WASM SIMD proposal (future)

## API Design

### TypeScript Interface (Target)

```typescript
import { PjsClient } from '@pjs/client';
import { WasmBackend } from '@pjs/client/wasm';

// Option 1: Explicit WASM backend
const client = new PjsClient({
  backend: new WasmBackend()
});

// Option 2: Auto-detect (WASM in browser, HTTP in Node.js)
const client = new PjsClient(); // Automatically uses WASM

// Parse JSON
const result = await client.parse(jsonString);
console.log(result.frames);

// Streaming API
const stream = client.createStream(largeJsonObject);
while (stream.hasMore()) {
  const frame = stream.nextFrame();
  renderFrame(frame); // Immediate UI update
  await new Promise(r => setTimeout(r, 0)); // Yield to browser
}

// Schema validation
const schema = {
  type: "object",
  required: ["id", "name"],
  properties: {
    id: { type: "number" },
    name: { type: "string" }
  }
};

const validationResult = client.validate(data, schema);
if (!validationResult.valid) {
  console.error(validationResult.errors);
}
```

## Testing Strategy

### Unit Tests (Rust)

```bash
# Test domain logic
cargo test -p pjs-domain

# Test WASM bindings (headless browser)
cd crates/pjs-wasm
wasm-pack test --headless --firefox
wasm-pack test --headless --chrome
```

### Integration Tests (pjs-js-client)

```bash
# Run WASM tests in pjs-js-client
cd crates/pjs-js-client
npm test -- wasm.test.ts

# Run all tests with WASM backend
npm test -- --env=wasm
```

### Performance Benchmarks

```rust
#[wasm_bindgen]
pub fn benchmark_parse(json: &str, iterations: u32) -> f64 {
    let start = js_sys::Date::now();

    for _ in 0..iterations {
        let _ = parser.parse(json);
    }

    (js_sys::Date::now() - start) / iterations as f64
}
```

## CI/CD Pipeline

### GitHub Actions Workflow

```yaml
name: WASM Build

on: [push, pull_request]

jobs:
  test-wasm:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust nightly
        uses: dtolnay/rust-toolchain@nightly
        with:
          targets: wasm32-unknown-unknown

      - name: Install wasm-pack
        run: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

      - name: Test pjs-domain
        run: cargo test -p pjs-domain

      - name: Build WASM
        run: |
          cd crates/pjs-wasm
          wasm-pack build --target web

      - name: Run WASM tests
        run: |
          cd crates/pjs-wasm
          wasm-pack test --headless --firefox

      - name: Test pjs-js-client with WASM
        run: |
          cd crates/pjs-js-client
          npm install
          npm test

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: pjs-wasm-pkg
          path: crates/pjs-wasm/pkg/
```

## Documentation Requirements

### Required Documentation

1. **WASM Getting Started Guide** (`docs/guides/wasm-quickstart.md`)
   - Installation instructions
   - Basic usage examples
   - Browser compatibility

2. **API Reference** (`docs/api/wasm-api.md`)
   - Complete TypeScript API documentation
   - Code examples for each method
   - Error handling patterns

3. **pjs-js-client Integration Guide** (`crates/pjs-js-client/README.md`)
   - Using WASM backend
   - Performance considerations
   - When to use WASM vs HTTP

4. **Migration Guide** (`docs/guides/wasm-migration.md`)
   - Converting from HTTP to WASM backend
   - Performance considerations
   - Common pitfalls

5. **Architecture Decision Record** (`docs/architecture/adr/001-wasm-support.md`)
   - Why separate crates
   - Technical trade-offs
   - Future considerations

## Risks and Mitigation

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Performance too slow | High | Medium | Benchmark early, set expectations |
| Bundle size too large | Medium | Low | Use `opt-level="s"`, tree shaking |
| Browser compatibility | Medium | Low | Test Firefox, Chrome, Safari |
| Maintenance overhead | High | High | Automate testing, shared domain logic |
| GAT feature instability | Low | Low | WASM supports nightly |
| pjs-js-client breaking changes | Medium | Medium | Version lock, comprehensive tests |

## Success Metrics

### Phase 1-2 (Foundation)

- ✅ `pjs-domain` crate compiles independently
- ✅ `pjs-wasm` generates valid npm package
- ✅ TypeScript types are correct

### Phase 3-4 (Core Functionality & Integration)

- ✅ Parse 100KB JSON in < 10ms
- ✅ pjs-js-client tests pass with WASM backend
- ✅ Feature detection works correctly

### Phase 5-7 (Production Ready)

- ✅ Browser demo works in all major browsers
- ✅ npm package published
- ✅ pjs-js-client uses published WASM module
- ✅ CI/CD pipeline operational
- ✅ Documentation complete
- ✅ At least 80% test coverage

## Timeline

| Phase | Duration | Start | End | Status |
|-------|----------|-------|-----|--------|
| Phase 1: Extract Domain | 1 week | Week 1 | Week 1 | Pending |
| Phase 2: WASM Crate Setup | 1 week | Week 2 | Week 2 | Pending |
| Phase 3: Core Parsing | 1 week | Week 3 | Week 3 | Pending |
| Phase 4: pjs-js-client Integration | 1 week | Week 4 | Week 4 | Pending |
| Phase 5: Browser Demo | 1 week | Week 5 | Week 5 | Pending |
| Phase 6: Advanced Features | 2 weeks | Week 6 | Week 7 | Pending |
| Phase 7: Publishing & CI/CD | 1 week | Week 8 | Week 8 | Pending |
| **Total** | **8 weeks** | - | - | - |

## Next Steps

### Immediate Actions (Next 48 Hours)

1. ✅ Create feature branch: `feature/wasm-support`
2. ✅ Move SPECIFICATION.md to `docs/architecture/`
3. ✅ Create this roadmap document
4. ⏳ Begin Phase 1: Extract `pjs-domain` crate
5. ⏳ Set up initial directory structure

### Week 1 Goals

- Complete `pjs-domain` crate extraction
- Ensure all domain tests pass
- Verify no external dependencies
- Prepare for Phase 2

## References

### External Resources

- [wasm-bindgen Book](https://rustwasm.github.io/wasm-bindgen/)
- [wasm-pack Documentation](https://rustwasm.github.io/wasm-pack/)
- [Rust WASM Book](https://rustwasm.github.io/book/)
- [WebAssembly SIMD Proposal](https://github.com/WebAssembly/simd)
- [Microsoft Rust Guidelines](https://microsoft.github.io/rust-guidelines/)

### Internal Documents

- [SPECIFICATION.md](SPECIFICATION.md) - PJS protocol specification
- [CLAUDE.md](../guides/CLAUDE.md) - Project development guidelines
- [pjs-js-client README](../../crates/pjs-js-client/README.md) - JS client documentation

## Appendix A: Dependencies

### pjs-domain Dependencies

```toml
[dependencies]
serde = { version = "1.0", features = ["derive"], optional = true }
thiserror = { version = "2.0", default-features = false }

[features]
default = ["std"]
std = ["thiserror/std"]
serde = ["dep:serde"]
```

### pjs-wasm Dependencies

```toml
[dependencies]
pjs-domain = { path = "../pjs-domain", default-features = false }
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
js-sys = "0.3"
web-sys = { version = "0.3", features = ["console", "WebSocket"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde-wasm-bindgen = "0.6"
once_cell = { version = "1.21", default-features = false, features = ["alloc"] }

[dev-dependencies]
wasm-bindgen-test = "0.3"
```

### pjs-js-client Updates

```json
{
  "dependencies": {
    "@pjs/wasm": "^0.1.0"
  },
  "devDependencies": {
    "@types/wasm-bindgen": "^0.2.0"
  }
}
```

## Appendix B: File Structure

```
docs/
├── architecture/
│   ├── SPECIFICATION.md          # Protocol specification
│   ├── WASM_ROADMAP.md          # This document
│   └── adr/                     # Architecture Decision Records
│       └── 001-wasm-support.md
├── guides/
│   ├── CLAUDE.md                # Development guidelines
│   ├── wasm-quickstart.md       # WASM getting started
│   └── wasm-migration.md        # Migration guide
└── api/
    └── wasm-api.md              # TypeScript API reference

crates/
├── pjs-domain/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs
│   │   ├── value_objects/
│   │   ├── entities/
│   │   ├── events/
│   │   └── services/
│   └── tests/
├── pjs-wasm/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs
│   │   ├── parser.rs
│   │   ├── api.rs
│   │   └── types.rs
│   ├── tests/
│   └── pkg/                     # Generated by wasm-pack
└── pjs-js-client/
    ├── package.json
    ├── src/
    │   ├── wasm/
    │   │   ├── wasm-backend.ts
    │   │   └── index.ts
    │   └── index.ts
    └── tests/
        └── wasm.test.ts

examples/
└── wasm-demo/
    ├── index.html
    ├── style.css
    ├── main.js
    └── README.md
```

---

**Document Status**: Draft
**Approval Required**: Yes
**Next Review**: After Phase 1 completion
