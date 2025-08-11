/**
 * WebSocket Transport - WebSocket-based transport for PJS protocol
 * 
 * Implements real-time streaming over WebSocket connections
 * with automatic reconnection and frame multiplexing.
 */

import { Transport, ConnectResult, StreamOptions } from './base.js';
import { Frame, PJSError, PJSErrorType } from '../types/index.js';

/**
 * WebSocket transport implementation
 */
export class WebSocketTransport extends Transport {
  private ws?: WebSocket;
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 5;
  private reconnectDelay = 1000;

  async connect(): Promise<ConnectResult> {
    return new Promise((resolve, reject) => {
      try {
        const wsUrl = this.config.baseUrl.replace(/^http/, 'ws') + '/pjs/ws';
        this.ws = new WebSocket(wsUrl);
        
        this.ws.onopen = () => {
          this.isConnected = true;
          this.reconnectAttempts = 0;
          
          if (this.config.debug) {
            console.log('[PJS WebSocket] Connected to:', wsUrl);
          }
          
          resolve({
            sessionId: undefined, // Will be provided by server
            supportedFeatures: ['realtime', 'multiplexing']
          });
        };
        
        this.ws.onmessage = (event) => {
          this.handleMessage(event.data);
        };
        
        this.ws.onerror = (error) => {
          const pjsError = new PJSError(
            PJSErrorType.ConnectionError,
            'WebSocket connection error',
            { error }
          );
          
          if (!this.isConnected) {
            reject(pjsError);
          } else {
            this.emitError(pjsError);
          }
        };
        
        this.ws.onclose = (event) => {
          this.isConnected = false;
          
          if (event.code !== 1000 && this.reconnectAttempts < this.maxReconnectAttempts) {
            // Attempt reconnection
            setTimeout(() => {
              this.reconnectAttempts++;
              this.connect().catch(() => {
                this.emitDisconnect();
              });
            }, this.reconnectDelay * Math.pow(2, this.reconnectAttempts));
          } else {
            this.emitDisconnect();
          }
        };
        
      } catch (error) {
        reject(new PJSError(
          PJSErrorType.ConnectionError,
          'Failed to create WebSocket connection',
          undefined,
          error as Error
        ));
      }
    });
  }

  async disconnect(): Promise<void> {
    if (this.ws) {
      this.ws.close(1000, 'Client disconnect');
      this.ws = undefined;
    }
    
    this.isConnected = false;
  }

  async startStream(endpoint: string, options: StreamOptions): Promise<void> {
    if (!this.ws || !this.isConnected) {
      throw new PJSError(
        PJSErrorType.ConnectionError,
        'WebSocket not connected'
      );
    }

    const message = {
      type: 'start_stream',
      endpoint,
      session_id: options.sessionId,
      stream_id: options.streamId,
      query_params: options.queryParams,
      headers: options.headers
    };

    this.ws.send(JSON.stringify(message));
  }

  async stopStream(): Promise<void> {
    if (!this.ws || !this.isConnected) {
      return;
    }

    const message = {
      type: 'stop_stream'
    };

    this.ws.send(JSON.stringify(message));
  }

  private handleMessage(data: string): void {
    try {
      const message = JSON.parse(data);
      
      if (message.type === 'frame') {
        this.emitFrame(message.frame as Frame);
      } else if (message.type === 'error') {
        this.emitError(new PJSError(
          PJSErrorType.ProtocolError,
          message.error || 'Server error',
          message
        ));
      } else if (this.config.debug) {
        console.log('[PJS WebSocket] Unknown message type:', message.type);
      }
      
    } catch (error) {
      this.emitError(new PJSError(
        PJSErrorType.ParseError,
        'Failed to parse WebSocket message',
        { data },
        error as Error
      ));
    }
  }
}