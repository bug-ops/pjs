/**
 * Base Transport - Abstract base class for PJS transport implementations
 * 
 * Defines the common interface for HTTP, WebSocket, and SSE transports.
 */

import { EventEmitter } from 'events';
import { Frame, JsonValue, PJSClientConfig, PJSError, PJSErrorType } from '../types/index.js';

/** Name of the SSE event (and NDJSON marker key) a server sends before closing on shutdown. */
export const SHUTDOWN_EVENT = 'shutdown';

/** Type guard for the server shutdown marker `{ "event": "shutdown" }`. */
export function isShutdownMarker(value: unknown): value is { event: typeof SHUTDOWN_EVENT } {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return false;
  }
  const keys = Object.keys(value);
  return keys.length === 1 && keys[0] === 'event' && (value as { event: unknown }).event === SHUTDOWN_EVENT;
}

export interface ConnectResult {
  sessionId?: string;
  supportedFeatures?: string[];
}

export interface StreamOptions {
  sessionId: string;
  streamId: string;
  /** JSON document to stream (required by the WebSocket transport). */
  data?: JsonValue;
  queryParams?: Record<string, string>;
  headers?: Record<string, string>;
}

/**
 * Abstract base transport class
 */
export abstract class Transport extends EventEmitter {
  protected config: Required<PJSClientConfig>;
  protected isConnected = false;
  
  constructor(config: Required<PJSClientConfig>) {
    super();
    this.config = config;
  }

  /**
   * Connect to PJS server
   */
  abstract connect(): Promise<ConnectResult>;

  /**
   * Disconnect from server
   */
  abstract disconnect(): Promise<void>;

  /**
   * Start streaming from an endpoint
   */
  abstract startStream(endpoint: string, options: StreamOptions): Promise<void>;

  /**
   * Stop current stream
   */
  abstract stopStream(): Promise<void>;

  /**
   * Check if transport is connected
   */
  isTransportConnected(): boolean {
    return this.isConnected;
  }

  /**
   * Emit frame event (used by implementations)
   */
  protected emitFrame(frame: Frame): void {
    this.emit('frame', frame);
  }

  /**
   * Emit error event (used by implementations)
   */
  protected emitError(error: Error): void {
    this.emit('error', error);
  }

  /**
   * Emit disconnect event (used by implementations)
   */
  protected emitDisconnect(): void {
    this.emit('disconnect');
  }

  /**
   * Emit server shutdown event (used by implementations)
   */
  protected emitShutdown(): void {
    // rawListeners keeps once() wrappers, so they unregister themselves as with emit().
    for (const listener of this.rawListeners(SHUTDOWN_EVENT)) {
      try {
        listener.call(this);
      } catch (error) {
        // A throwing listener must not abort the shutdown flow for the others.
        if (this.listenerCount('error') > 0) {
          this.emitError(new PJSError(
            PJSErrorType.ServerShutdown,
            'Shutdown listener threw',
            undefined,
            error as Error
          ));
        } else if (this.config.debug) {
          console.error('[PJS] Shutdown listener threw:', error);
        }
      }
    }
  }
}
