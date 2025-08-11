/**
 * Server-Sent Events Transport - SSE-based transport for PJS protocol
 * 
 * Implements streaming over Server-Sent Events with automatic reconnection
 * and event parsing for frame delivery.
 */

import { Transport, ConnectResult, StreamOptions } from './base.js';
import { Frame, PJSError, PJSErrorType } from '../types/index.js';

/**
 * Server-Sent Events transport implementation
 */
export class SSETransport extends Transport {
  private eventSource?: EventSource;
  private currentUrl?: string;

  async connect(): Promise<ConnectResult> {
    // SSE connection is established when starting stream
    this.isConnected = true;
    
    return {
      sessionId: undefined,
      supportedFeatures: ['streaming', 'auto-reconnect']
    };
  }

  async disconnect(): Promise<void> {
    if (this.eventSource) {
      this.eventSource.close();
      this.eventSource = undefined;
    }
    
    this.isConnected = false;
    this.currentUrl = undefined;
  }

  async startStream(endpoint: string, options: StreamOptions): Promise<void> {
    if (!this.isConnected) {
      throw new PJSError(
        PJSErrorType.ConnectionError,
        'Transport not connected'
      );
    }

    // Close existing connection
    if (this.eventSource) {
      this.eventSource.close();
    }

    try {
      const url = new URL(`${this.config.baseUrl}/pjs/stream${endpoint}/sse`);
      
      // Add query parameters
      url.searchParams.set('session_id', options.sessionId);
      url.searchParams.set('stream_id', options.streamId);
      
      if (options.queryParams) {
        Object.entries(options.queryParams).forEach(([key, value]) => {
          url.searchParams.set(key, value);
        });
      }

      this.currentUrl = url.toString();
      this.eventSource = new EventSource(this.currentUrl);
      
      this.setupEventHandlers();
      
    } catch (error) {
      throw new PJSError(
        PJSErrorType.ConnectionError,
        'Failed to start SSE stream',
        { endpoint, options },
        error as Error
      );
    }
  }

  async stopStream(): Promise<void> {
    if (this.eventSource) {
      this.eventSource.close();
      this.eventSource = undefined;
    }
    
    this.currentUrl = undefined;
  }

  private setupEventHandlers(): void {
    if (!this.eventSource) return;

    this.eventSource.onopen = () => {
      if (this.config.debug) {
        console.log('[PJS SSE] Stream opened:', this.currentUrl);
      }
    };

    this.eventSource.onmessage = (event) => {
      this.handleMessage(event.data);
    };

    this.eventSource.onerror = (error) => {
      if (this.config.debug) {
        console.log('[PJS SSE] Connection error, will reconnect automatically');
      }
      
      // EventSource will handle reconnection automatically
      this.emitError(new PJSError(
        PJSErrorType.ConnectionError,
        'SSE connection error',
        { error, url: this.currentUrl }
      ));
    };

    // Handle custom frame events
    this.eventSource.addEventListener('frame', (event) => {
      this.handleMessage((event as MessageEvent).data);
    });

    this.eventSource.addEventListener('error', (event) => {
      const data = (event as MessageEvent).data;
      try {
        const errorData = JSON.parse(data);
        this.emitError(new PJSError(
          PJSErrorType.ProtocolError,
          errorData.message || 'Server error',
          errorData
        ));
      } catch {
        this.emitError(new PJSError(
          PJSErrorType.ProtocolError,
          'Server error',
          { data }
        ));
      }
    });
  }

  private handleMessage(data: string): void {
    try {
      const frame = JSON.parse(data) as Frame;
      this.emitFrame(frame);
      
    } catch (error) {
      this.emitError(new PJSError(
        PJSErrorType.ParseError,
        'Failed to parse SSE frame',
        { data },
        error as Error
      ));
    }
  }
}