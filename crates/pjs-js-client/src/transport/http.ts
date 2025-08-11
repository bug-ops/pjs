/**
 * HTTP Transport - HTTP-based transport for PJS protocol
 * 
 * Implements streaming over HTTP using chunked transfer encoding
 * and Server-Sent Events for frame delivery.
 */

import { Transport, ConnectResult, StreamOptions } from './base.js';
import { Frame, PJSError, PJSErrorType } from '../types/index.js';

/**
 * HTTP transport implementation
 */
export class HttpTransport extends Transport {
  private abortController?: AbortController;
  private currentStreamEndpoint?: string;

  async connect(): Promise<ConnectResult> {
    try {
      const response = await this.makeRequest('/pjs/sessions', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          ...this.config.headers
        },
        body: JSON.stringify({
          client_info: {
            type: 'javascript',
            version: '0.3.0-alpha.1'
          }
        })
      });

      if (!response.ok) {
        throw new PJSError(
          PJSErrorType.ConnectionError,
          `HTTP ${response.status}: ${response.statusText}`
        );
      }

      const data = await response.json();
      this.isConnected = true;
      
      return {
        sessionId: data.session_id,
        supportedFeatures: data.supported_features || []
      };
      
    } catch (error) {
      if (error instanceof PJSError) {
        throw error;
      }
      
      throw new PJSError(
        PJSErrorType.ConnectionError,
        'Failed to connect via HTTP',
        undefined,
        error as Error
      );
    }
  }

  async disconnect(): Promise<void> {
    if (this.abortController) {
      this.abortController.abort();
      this.abortController = undefined;
    }
    
    this.isConnected = false;
    this.currentStreamEndpoint = undefined;
  }

  async startStream(endpoint: string, options: StreamOptions): Promise<void> {
    if (!this.isConnected) {
      throw new PJSError(
        PJSErrorType.ConnectionError,
        'Transport not connected'
      );
    }

    // Cancel any existing stream
    if (this.abortController) {
      this.abortController.abort();
    }

    this.abortController = new AbortController();
    this.currentStreamEndpoint = endpoint;

    try {
      const url = new URL(`${this.config.baseUrl}/pjs/stream${endpoint}`);
      
      // Add query parameters
      url.searchParams.set('session_id', options.sessionId);
      url.searchParams.set('stream_id', options.streamId);
      
      if (options.queryParams) {
        Object.entries(options.queryParams).forEach(([key, value]) => {
          url.searchParams.set(key, value);
        });
      }

      const response = await this.makeRequest(url.toString(), {
        method: 'GET',
        headers: {
          'Accept': 'application/x-ndjson',
          'Cache-Control': 'no-cache',
          ...this.config.headers,
          ...options.headers
        },
        signal: this.abortController.signal
      });

      if (!response.ok) {
        throw new PJSError(
          PJSErrorType.ConnectionError,
          `HTTP ${response.status}: ${response.statusText}`
        );
      }

      // Process streaming response
      await this.processStreamingResponse(response);
      
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') {
        // Stream was cancelled, this is expected
        return;
      }
      
      if (error instanceof PJSError) {
        this.emitError(error);
        throw error;
      }
      
      const pjsError = new PJSError(
        PJSErrorType.ConnectionError,
        'Failed to start HTTP stream',
        { endpoint, options },
        error as Error
      );
      
      this.emitError(pjsError);
      throw pjsError;
    }
  }

  async stopStream(): Promise<void> {
    if (this.abortController) {
      this.abortController.abort();
      this.abortController = undefined;
    }
    
    this.currentStreamEndpoint = undefined;
  }

  // Private methods

  private async makeRequest(url: string, options: RequestInit): Promise<Response> {
    const requestOptions: RequestInit = {
      ...options,
      signal: options.signal || AbortSignal.timeout(this.config.timeout)
    };

    if (this.config.debug) {
      console.log('[PJS HTTP] Making request:', { url, method: options.method });
    }

    return fetch(url, requestOptions);
  }

  private async processStreamingResponse(response: Response): Promise<void> {
    const reader = response.body?.getReader();
    if (!reader) {
      throw new PJSError(
        PJSErrorType.ConnectionError,
        'Response body is not readable'
      );
    }

    const decoder = new TextDecoder();
    let buffer = '';

    try {
      while (true) {
        const { done, value } = await reader.read();
        
        if (done) {
          break;
        }

        // Decode chunk and add to buffer
        buffer += decoder.decode(value, { stream: true });
        
        // Process complete lines (NDJSON format)
        const lines = buffer.split('\n');
        buffer = lines.pop() || ''; // Keep incomplete line in buffer
        
        for (const line of lines) {
          if (line.trim()) {
            try {
              const frame = JSON.parse(line) as Frame;
              this.emitFrame(frame);
            } catch (error) {
              this.emitError(new PJSError(
                PJSErrorType.ParseError,
                'Failed to parse frame from HTTP response',
                { line },
                error as Error
              ));
            }
          }
        }
      }
      
      // Process any remaining data in buffer
      if (buffer.trim()) {
        try {
          const frame = JSON.parse(buffer) as Frame;
          this.emitFrame(frame);
        } catch (error) {
          this.emitError(new PJSError(
            PJSErrorType.ParseError,
            'Failed to parse final frame from HTTP response',
            { buffer },
            error as Error
          ));
        }
      }
      
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') {
        // Stream was cancelled
        return;
      }
      
      throw error;
      
    } finally {
      reader.releaseLock();
    }
  }
}