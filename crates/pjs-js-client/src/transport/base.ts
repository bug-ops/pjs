/**
 * Base Transport - Abstract base class for PJS transport implementations
 * 
 * Defines the common interface for HTTP, WebSocket, and SSE transports.
 */

import { EventEmitter } from 'events';
import { Frame, PJSClientConfig } from '../types/index.js';

/** Name of the SSE event (and NDJSON marker key) a server sends before closing on shutdown. */
export const SHUTDOWN_EVENT = 'shutdown';

/** Type guard for the server shutdown marker `{ "event": "shutdown" }`. */
export function isShutdownMarker(_v: unknown): _v is { event: typeof SHUTDOWN_EVENT } {
  throw new Error('not implemented');
}

export interface ConnectResult {
  sessionId?: string;
  supportedFeatures?: string[];
}

export interface StreamOptions {
  sessionId: string;
  streamId: string;
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
    throw new Error('not implemented');
  }
}
