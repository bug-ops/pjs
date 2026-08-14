/**
 * WasmParser Integration Tests
 *
 * Regression coverage for issue #338: WasmParser.stream() used to throw
 * immediately because `this.wasmModule` was never assigned in initialize().
 */

import { describe, test, expect, beforeEach, afterEach } from '@jest/globals';
import { existsSync } from 'fs';
import { resolve } from 'path';
import { WasmParser } from '../../src/parser/wasm-parser.js';
import { FrameType } from '../../src/types/index.js';

// Skip the entire suite when pjs-wasm/pkg is not built
const wasmPkgAvailable = existsSync(resolve(process.cwd(), 'crates/pjs-wasm/pkg/package.json'))
  || existsSync(resolve(process.cwd(), '../pjs-wasm/pkg/package.json'));
const describeWasm = wasmPkgAvailable ? describe : describe.skip;

describeWasm('WasmParser Integration Tests', () => {
  let parser: WasmParser;

  beforeEach(() => {
    parser = new WasmParser();
  });

  afterEach(() => {
    parser.dispose();
  });

  test('should initialize WASM module successfully', async () => {
    const initialized = await parser.initialize();

    expect(initialized).toBe(true);
    expect(parser.isWasmAvailable()).toBe(true);
    expect(parser.getImplementation()).toBe('wasm');
  });

  test('should stream JSON with priority-based progressive delivery', async () => {
    await parser.initialize();

    const jsonData = JSON.stringify({
      id: 123,
      name: 'Alice',
      email: 'alice@example.com',
      bio: 'Software developer'
    });

    const frames: FrameType[] = [];
    let completeStats: { totalFrames: number; patchFrames: number; durationMs: number } | undefined;

    await parser.stream(
      jsonData,
      {
        onFrame: (frame) => {
          frames.push(frame.type);
        },
        onComplete: (stats) => {
          completeStats = stats;
        },
        onError: (error) => {
          throw error;
        }
      },
      1
    );

    expect(frames.length).toBeGreaterThan(0);
    expect(frames).toContain(FrameType.Skeleton);
    expect(completeStats).toBeDefined();
    expect(completeStats?.totalFrames).toBeGreaterThan(0);
  });
});

// Does not require the pjs-wasm binary — runs unconditionally. Exercises the
// early-return native-fallback guard, not the #338 bug itself; see
// tests/parser/wasm-parser.test.ts for #338's actual regression coverage.
describe('WasmParser (native fallback)', () => {
  test('should reject stream() when WASM is not available', async () => {
    const nativeParser = new WasmParser({ preferNative: true });
    await nativeParser.initialize();

    expect(nativeParser.isWasmAvailable()).toBe(false);
    await expect(
      nativeParser.stream('{"test": true}', {})
    ).rejects.toThrow('WASM not available');
  });
});
