/**
 * WasmParser Unit Tests (mocked pjs-wasm)
 *
 * Regression coverage for issue #338: WasmParser.stream() destructured
 * `this.wasmModule`, a field `initialize()` never assigned, so every call
 * threw immediately. Mocks `loadWasmModule()` so this exercises the actual
 * broken code path — `initialize()` storing the module, then `stream()`
 * reading it back — without needing the real pjs-wasm binary, so it runs
 * unconditionally in CI regardless of whether pjs-wasm/pkg is built.
 */

import { describe, test, expect, jest } from '@jest/globals';

jest.mock('../../src/utils/wasm-loader.js', () => {
  class FakePriorityStream {
    private frameCallback: ((frame: unknown) => void) | null = null;
    private completeCallback: ((stats: unknown) => void) | null = null;

    onFrame(callback: (frame: unknown) => void): void {
      this.frameCallback = callback;
    }

    onComplete(callback: (stats: unknown) => void): void {
      this.completeCallback = callback;
    }

    onError(_callback: (error: string) => void): void {}

    setMinPriority(_priority: number): void {}

    start(jsonString: string): void {
      this.frameCallback?.({
        type: 'skeleton',
        sequence: 0,
        priority: 100,
        payload: jsonString
      });
      this.completeCallback?.({
        totalFrames: 1,
        patchFrames: 0,
        bytesProcessed: jsonString.length,
        durationMs: 1
      });
    }

    free(): void {}
  }

  class FakePjsParser {
    parse(jsonString: string): unknown {
      return JSON.parse(jsonString);
    }

    static version(): string {
      return '0.0.0-fake';
    }
  }

  return {
    loadWasmModule: jest.fn(async () => ({
      PjsParser: FakePjsParser,
      PriorityStream: FakePriorityStream,
      version: () => '0.0.0-fake',
      default: async () => undefined
    }))
  };
});

import { WasmParser } from '../../src/parser/wasm-parser.js';
import { FrameType } from '../../src/types/index.js';

describe('WasmParser (mocked pjs-wasm)', () => {
  test('stream() delivers frames after initialize() — this.wasmModule must be assigned', async () => {
    const parser = new WasmParser();

    const initialized = await parser.initialize();
    expect(initialized).toBe(true);
    expect(parser.isWasmAvailable()).toBe(true);

    const frameTypes: FrameType[] = [];
    let completed = false;

    await parser.stream(
      JSON.stringify({ id: 1 }),
      {
        onFrame: (frame) => {
          frameTypes.push(frame.frame_type);
        },
        onComplete: () => {
          completed = true;
        },
        onError: (error) => {
          throw error;
        }
      },
      1
    );

    expect(frameTypes).toContain(FrameType.Skeleton);
    expect(completed).toBe(true);
  });
});
