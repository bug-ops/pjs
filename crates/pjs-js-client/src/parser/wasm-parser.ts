/**
 * WASM Parser Integration
 *
 * Optional WebAssembly-accelerated JSON parsing using pjs-wasm.
 * Falls back to native JSON.parse() if WASM is not available.
 *
 * Supports both simple parsing and priority-based streaming:
 * - parse(): Single-call JSON parsing
 * - stream(): Progressive frame delivery with callbacks
 */

import { PJSError, PJSErrorType, Frame, FrameType } from '../types/index.js';
import { loadWasmModule } from '../utils/wasm-loader.js';
import type { PriorityStream, FrameData, StreamStats } from 'pjs-wasm';

export interface WasmParserOptions {
  debug?: boolean;
  preferNative?: boolean; // Force use of native JSON.parse
}

export interface StreamingCallbacks {
  onFrame?: (frame: Frame) => void;
  onComplete?: (stats: { totalFrames: number; patchFrames: number; durationMs: number }) => void;
  onError?: (error: Error) => void;
}

/**
 * WASM Parser instance
 *
 * Provides high-performance JSON parsing using Rust-based WASM module.
 */
export class WasmParser {
  private parser: any = null;
  private wasmModule: typeof import('pjs-wasm') | null = null;
  private wasmAvailable = false;
  private initError: Error | null = null;
  private options: Required<WasmParserOptions>;

  constructor(options: WasmParserOptions = {}) {
    this.options = {
      debug: false,
      preferNative: false,
      ...options
    };
  }

  /**
   * Initialize WASM module (async)
   *
   * @returns true if WASM initialized successfully, false if falling back to native
   */
  async initialize(): Promise<boolean> {
    if (this.options.preferNative) {
      if (this.options.debug) {
        console.log('[PJS WASM] Using native JSON.parse (preferNative=true)');
      }
      return false;
    }

    try {
      // Dynamic import - won't fail if pjs-wasm is not installed
      const wasmModule = await loadWasmModule();
      this.wasmModule = wasmModule;

      // Create parser instance
      this.parser = new wasmModule.PjsParser();
      this.wasmAvailable = true;

      if (this.options.debug) {
        const version = wasmModule.version();
        console.log(`[PJS WASM] Initialized successfully (v${version})`);
      }

      return true;

    } catch (error) {
      if (this.options.debug) {
        console.warn('[PJS WASM] Not available, falling back to native JSON.parse:', error);
      }
      this.wasmAvailable = false;
      this.initError = error as Error;
      return false;
    }
  }

  /**
   * Parse JSON string
   *
   * @param jsonString - JSON string to parse
   * @returns Parsed JSON object
   * @throws PJSError if parsing fails
   */
  parse<T = any>(jsonString: string): T {
    try {
      if (this.wasmAvailable && this.parser) {
        // Use WASM parser
        return this.parser.parse(jsonString);
      } else {
        // Fallback to native JSON.parse
        return JSON.parse(jsonString);
      }
    } catch (error) {
      throw new PJSError(
        PJSErrorType.ParseError,
        'Failed to parse JSON',
        { inputLength: jsonString.length },
        error as Error
      );
    }
  }

  /**
   * Check if WASM is available and initialized
   */
  isWasmAvailable(): boolean {
    return this.wasmAvailable;
  }

  /**
   * Get parser implementation name
   */
  getImplementation(): 'wasm' | 'native' {
    return this.wasmAvailable ? 'wasm' : 'native';
  }

  /**
   * Stream JSON with priority-based progressive delivery
   *
   * Uses PriorityStream from pjs-wasm for callback-based frame delivery.
   *
   * @param jsonString - JSON string to stream
   * @param callbacks - Callbacks for frame, complete, and error events
   * @param minPriority - Minimum priority threshold (1-255, default 1)
   *
   * @example
   * ```typescript
   * const parser = await createWasmParser();
   *
   * await parser.stream(largeJson, {
   *   onFrame: (frame) => {
   *     console.log('Frame:', frame.frame_type, 'priority:', frame.priority);
   *     updateUI(frame);
   *   },
   *   onComplete: (stats) => {
   *     console.log('Complete!', stats);
   *   },
   *   onError: (error) => {
   *     console.error('Stream error:', error);
   *   }
   * }, 50); // Only frames with priority >= 50
   * ```
   */
  async stream(
    jsonString: string,
    callbacks: StreamingCallbacks,
    minPriority: number = 1
  ): Promise<void> {
    if (!this.wasmAvailable || !this.wasmModule) {
      throw new PJSError(
        PJSErrorType.InitializationError,
        this.initError
          ? `WASM not available. Cannot use streaming API without WASM: ${this.initError.message}`
          : 'WASM not available. Cannot use streaming API without WASM.',
        undefined,
        this.initError ?? undefined
      );
    }

    const wasmModule = this.wasmModule;

    return new Promise<void>((resolve, reject) => {
      try {
        const { PriorityStream } = wasmModule;
        const stream: PriorityStream = new PriorityStream();

        // Set minimum priority
        stream.setMinPriority(minPriority);

        // Set up callbacks
        stream.onFrame((frameData: FrameData) => {
          if (callbacks.onFrame) {
            const frame = this.convertWasmFrameToFrame(frameData);
            callbacks.onFrame(frame);
          }
        });

        stream.onComplete((stats: StreamStats) => {
          if (callbacks.onComplete) {
            callbacks.onComplete({
              totalFrames: stats.totalFrames,
              patchFrames: stats.patchFrames,
              durationMs: stats.durationMs
            });
          }

          // Free WASM resources
          if (typeof stream.free === 'function') {
            stream.free();
          }

          resolve();
        });

        stream.onError((error: string) => {
          const pjsError = new PJSError(
            PJSErrorType.StreamError,
            `WASM streaming error: ${error}`,
            { jsonLength: jsonString.length }
          );

          if (callbacks.onError) {
            callbacks.onError(pjsError);
          }

          // Free WASM resources
          if (typeof stream.free === 'function') {
            stream.free();
          }

          reject(pjsError);
        });

        // Start streaming
        stream.start(jsonString);

      } catch (error) {
        const pjsError = error instanceof PJSError
          ? error
          : new PJSError(
              PJSErrorType.StreamError,
              'Failed to start WASM streaming',
              { error: (error as Error).message }
            );

        if (callbacks.onError) {
          callbacks.onError(pjsError);
        }

        reject(pjsError);
      }
    });
  }

  /**
   * Convert WASM FrameData to PJS Frame format
   */
  private convertWasmFrameToFrame(frameData: FrameData): Frame {
    const frameType = this.mapFrameType(frameData.type);

    // Parse payload
    let payload: any;
    try {
      payload = JSON.parse(frameData.payload);
    } catch (error) {
      throw new PJSError(
        PJSErrorType.ParseError,
        'Failed to parse WASM frame payload',
        { frameType: frameData.type }
      );
    }

    // Convert to PJS Frame format
    if (frameType === FrameType.Skeleton) {
      return {
        frame_type: FrameType.Skeleton,
        priority: frameData.priority,
        sequence: Number(frameData.sequence),
        payload,
        complete: false,
        metadata: {
          source: 'wasm-parser'
        }
      };
    } else if (frameType === FrameType.Patch) {
      return {
        frame_type: FrameType.Patch,
        priority: frameData.priority,
        sequence: Number(frameData.sequence),
        payload: { patches: payload.patches || [] },
        metadata: {
          source: 'wasm-parser'
        }
      };
    } else {
      return {
        frame_type: FrameType.Complete,
        priority: frameData.priority,
        sequence: Number(frameData.sequence),
        metadata: {
          source: 'wasm-parser'
        }
      };
    }
  }

  /**
   * Map WASM frame type string to PJS FrameType enum
   */
  private mapFrameType(wasmType: string): FrameType {
    switch (wasmType.toLowerCase()) {
      case 'skeleton':
        return FrameType.Skeleton;
      case 'patch':
        return FrameType.Patch;
      case 'complete':
        return FrameType.Complete;
      default:
        throw new PJSError(
          PJSErrorType.ValidationError,
          `Unknown WASM frame type: ${wasmType}`
        );
    }
  }

  /**
   * Dispose of WASM resources
   */
  dispose(): void {
    if (this.parser && typeof this.parser.free === 'function') {
      this.parser.free();
      this.parser = null;
    }
    this.wasmAvailable = false;
  }
}

/**
 * Create and initialize a WASM parser
 *
 * @param options - Parser options
 * @returns Initialized parser instance
 *
 * @example
 * ```typescript
 * const parser = await createWasmParser({ debug: true });
 * const data = parser.parse('{"name": "test"}');
 * console.log(`Using ${parser.getImplementation()} parser`);
 * ```
 */
export async function createWasmParser(
  options: WasmParserOptions = {}
): Promise<WasmParser> {
  const parser = new WasmParser(options);
  await parser.initialize();
  return parser;
}
