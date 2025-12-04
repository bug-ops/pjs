/**
 * WASM Backend Transport
 *
 * Local-first transport that uses WebAssembly for zero-latency JSON streaming.
 * This transport doesn't communicate with a server - instead it uses pjs-wasm
 * to parse and stream JSON data directly in the browser/Node.js.
 *
 * Use this for:
 * - Client-side JSON processing without server roundtrip
 * - Offline progressive rendering
 * - Embedding large JSON in the page with priority-based loading
 *
 * @example
 * ```typescript
 * import { PJSClient, TransportType } from '@pjs/client';
 *
 * const client = new PJSClient({
 *   baseUrl: 'wasm://local', // Special URL for WASM backend
 *   transport: TransportType.WASM
 * });
 *
 * // Stream JSON data locally (no network)
 * const data = await client.stream('embedded-data', {
 *   jsonData: largeJsonString,
 *   onRender: (partial) => updateUI(partial)
 * });
 * ```
 */

import { Transport, ConnectResult, StreamOptions } from './base.js';
import { Frame, FrameType, PJSClientConfig, PJSError, PJSErrorType } from '../types/index.js';
import type { PriorityStream, FrameData, StreamStats } from 'pjs-wasm';

/**
 * Options for WASM backend streaming
 */
export interface WasmStreamOptions extends StreamOptions {
  /**
   * JSON data to stream (required for WASM backend)
   */
  jsonData: string;

  /**
   * Minimum priority threshold (1-255)
   * @default 1
   */
  minPriority?: number;
}

/**
 * WASM Backend Transport
 *
 * Provides client-side JSON streaming using WebAssembly, with no server communication.
 */
export class WasmBackend extends Transport {
  private wasmModule: any = null;
  private currentStream: PriorityStream | null = null;
  private wasmAvailable = false;

  constructor(config: Required<PJSClientConfig>) {
    super(config);
  }

  /**
   * Initialize WASM module
   *
   * Unlike network transports, this loads the WASM module instead of
   * connecting to a server.
   */
  async connect(): Promise<ConnectResult> {
    if (this.isConnected) {
      return {
        sessionId: 'wasm-local',
        supportedFeatures: ['wasm', 'local-streaming', 'priority-streaming']
      };
    }

    try {
      // Dynamic import of pjs-wasm
      this.wasmModule = await import('pjs-wasm');

      // Initialize WASM (automatically called via wasm_bindgen(start))
      await this.wasmModule.default();

      this.wasmAvailable = true;
      this.isConnected = true;

      if (this.config.debug) {
        const version = this.wasmModule.version();
        console.log(`[PJS WASM Backend] Initialized (v${version})`);
      }

      return {
        sessionId: 'wasm-local',
        supportedFeatures: ['wasm', 'local-streaming', 'priority-streaming']
      };

    } catch (error) {
      throw new PJSError(
        PJSErrorType.InitializationError,
        'Failed to initialize WASM backend. Ensure pjs-wasm is installed.',
        { error: (error as Error).message }
      );
    }
  }

  /**
   * Disconnect WASM backend (cleanup resources)
   */
  async disconnect(): Promise<void> {
    if (this.currentStream) {
      // Free WASM resources
      if (typeof this.currentStream.free === 'function') {
        this.currentStream.free();
      }
      this.currentStream = null;
    }

    this.isConnected = false;
    this.emitDisconnect();

    if (this.config.debug) {
      console.log('[PJS WASM Backend] Disconnected');
    }
  }

  /**
   * Start streaming JSON data locally using WASM
   *
   * @param endpoint - Stream identifier (not used for network, just for tracking)
   * @param options - Stream options including jsonData
   */
  async startStream(endpoint: string, options: WasmStreamOptions): Promise<void> {
    if (!this.isConnected) {
      throw new PJSError(
        PJSErrorType.ConnectionError,
        'WASM backend not initialized. Call connect() first.'
      );
    }

    if (!options.jsonData) {
      throw new PJSError(
        PJSErrorType.ValidationError,
        'jsonData is required for WASM backend streaming'
      );
    }

    if (this.currentStream) {
      throw new PJSError(
        PJSErrorType.StreamError,
        'A stream is already active. Call stopStream() first.'
      );
    }

    try {
      // Create priority stream
      const { PriorityStream } = this.wasmModule;
      this.currentStream = new PriorityStream();

      const minPriority = options.minPriority ?? 1;
      this.currentStream.setMinPriority(minPriority);

      // Set up callbacks
      this.currentStream.onFrame((frameData: FrameData) => {
        const frame = this.convertWasmFrame(frameData);
        this.emitFrame(frame);
      });

      this.currentStream.onComplete((stats: StreamStats) => {
        if (this.config.debug) {
          console.log('[PJS WASM Backend] Stream complete:', {
            totalFrames: stats.totalFrames,
            patchFrames: stats.patchFrames,
            bytesProcessed: stats.bytesProcessed,
            durationMs: stats.durationMs
          });
        }

        // Emit complete frame
        this.emitFrame({
          type: FrameType.Complete,
          priority: 1,
          total_frames: stats.totalFrames
        });
      });

      this.currentStream.onError((error: string) => {
        const pjsError = new PJSError(
          PJSErrorType.StreamError,
          `WASM streaming error: ${error}`,
          { endpoint, jsonLength: options.jsonData.length }
        );
        this.emitError(pjsError);
      });

      // Start streaming
      if (this.config.debug) {
        console.log('[PJS WASM Backend] Starting stream:', {
          endpoint,
          jsonLength: options.jsonData.length,
          minPriority
        });
      }

      this.currentStream.start(options.jsonData);

    } catch (error) {
      const pjsError = error instanceof PJSError
        ? error
        : new PJSError(
            PJSErrorType.StreamError,
            'Failed to start WASM stream',
            { endpoint, error: (error as Error).message }
          );

      this.emitError(pjsError);
      throw pjsError;
    }
  }

  /**
   * Stop current stream
   */
  async stopStream(): Promise<void> {
    if (this.currentStream) {
      // Free WASM resources
      if (typeof this.currentStream.free === 'function') {
        this.currentStream.free();
      }
      this.currentStream = null;

      if (this.config.debug) {
        console.log('[PJS WASM Backend] Stream stopped');
      }
    }
  }

  /**
   * Convert WASM FrameData to PJS Frame format
   */
  private convertWasmFrame(frameData: FrameData): Frame {
    const frameType = this.mapFrameType(frameData.type);

    // Parse payload
    let payload: any;
    try {
      // Use getPayloadObject() if available (zero-copy)
      payload = typeof frameData.getPayloadObject === 'function'
        ? frameData.getPayloadObject()
        : JSON.parse(frameData.payload);
    } catch (error) {
      throw new PJSError(
        PJSErrorType.ParseError,
        'Failed to parse WASM frame payload',
        { frameType: frameData.type, error: (error as Error).message }
      );
    }

    // Convert to PJS Frame format
    if (frameType === FrameType.Skeleton) {
      return {
        type: FrameType.Skeleton,
        priority: frameData.priority,
        data: payload,
        complete: false,
        metadata: {
          sequence: Number(frameData.sequence),
          source: 'wasm'
        }
      };
    } else if (frameType === FrameType.Patch) {
      return {
        type: FrameType.Patch,
        priority: frameData.priority,
        patches: payload.patches || [],
        metadata: {
          sequence: Number(frameData.sequence),
          source: 'wasm'
        }
      };
    } else {
      // Complete frame
      return {
        type: FrameType.Complete,
        priority: frameData.priority,
        metadata: {
          sequence: Number(frameData.sequence),
          source: 'wasm'
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
   * Check if WASM is available
   */
  isWasmAvailable(): boolean {
    return this.wasmAvailable;
  }

  /**
   * Get WASM module version
   */
  getWasmVersion(): string | null {
    return this.wasmModule && this.wasmModule.version
      ? this.wasmModule.version()
      : null;
  }
}

/**
 * Create and initialize WASM backend
 *
 * @param config - Client configuration
 * @returns Initialized WASM backend transport
 *
 * @example
 * ```typescript
 * const backend = await createWasmBackend({
 *   baseUrl: 'wasm://local',
 *   debug: true
 * });
 *
 * backend.on('frame', (frame) => {
 *   console.log('Frame:', frame);
 * });
 *
 * await backend.startStream('my-data', {
 *   jsonData: '{"users": [...]}',
 *   sessionId: 'local',
 *   streamId: 'stream-1'
 * });
 * ```
 */
export async function createWasmBackend(
  config: Required<PJSClientConfig>
): Promise<WasmBackend> {
  const backend = new WasmBackend(config);
  await backend.connect();
  return backend;
}
