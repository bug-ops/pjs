/**
 * PJS Client - Main client class for Priority JSON Streaming Protocol
 * 
 * This is the primary entry point for using PJS in JavaScript/TypeScript applications.
 * It handles connection management, frame processing, and progressive JSON reconstruction.
 */

import { EventEmitter } from 'events';
import {
  PJSClientConfig,
  TransportType,
  PJSEvent,
  PJSEventData,
  EventListener,
  StreamOptions,
  Frame,
  FrameType,
  Priority,
  PJSError,
  PJSErrorType,
  StreamStats,
  PerformanceMetrics,
  ProgressInfo
} from '../types/index.js';
import { FrameProcessor } from './frame-processor.js';
import { JsonReconstructor } from './json-reconstructor.js';
import { HttpTransport } from '../transport/http.js';
import { WebSocketTransport } from '../transport/websocket.js';
import { SSETransport } from '../transport/sse.js';
import { Transport } from '../transport/base.js';

/**
 * Main PJS client class providing high-level API for streaming JSON
 */
export class PJSClient extends EventEmitter {
  private config: Required<PJSClientConfig>;
  private transport: Transport;
  private frameProcessor: FrameProcessor;
  private jsonReconstructor: JsonReconstructor;
  private sessionId?: string;
  private isConnected = false;
  private streams = new Map<string, StreamStats>();
  private currentStreamId?: string;

  constructor(config: PJSClientConfig) {
    super();
    
    // Validate and set default configuration
    this.config = this.validateAndNormalizeConfig(config);
    
    // Initialize components
    this.frameProcessor = new FrameProcessor({
      debug: this.config.debug,
      priorityThreshold: this.config.priorityThreshold
    });
    
    this.jsonReconstructor = new JsonReconstructor({
      bufferSize: this.config.bufferSize,
      debug: this.config.debug
    });
    
    // Initialize transport based on configuration
    this.transport = this.createTransport();
    
    // Set up event handlers
    this.setupEventHandlers();
    
    if (this.config.debug) {
      console.log('[PJS] Client initialized with config:', this.config);
    }
  }

  /**
   * Connect to PJS server and establish session
   */
  async connect(): Promise<string> {
    try {
      if (this.isConnected) {
        throw new PJSError(
          PJSErrorType.ConnectionError,
          'Client is already connected'
        );
      }

      const sessionData = await this.transport.connect();
      this.sessionId = sessionData.sessionId || this.config.sessionId;
      this.isConnected = true;

      this.emit(PJSEvent.Connected, { sessionId: this.sessionId! });
      
      if (this.config.debug) {
        console.log('[PJS] Connected with session:', this.sessionId);
      }

      return this.sessionId!;
    } catch (error) {
      const pjsError = error instanceof PJSError 
        ? error 
        : new PJSError(
            PJSErrorType.ConnectionError, 
            'Failed to connect to PJS server',
            undefined,
            error as Error
          );
      
      this.emit(PJSEvent.Error, { error: pjsError, context: 'connect' });
      throw pjsError;
    }
  }

  /**
   * Disconnect from server
   */
  async disconnect(): Promise<void> {
    try {
      if (!this.isConnected) {
        return;
      }

      await this.transport.disconnect();
      this.isConnected = false;
      this.sessionId = undefined;
      this.streams.clear();
      this.currentStreamId = undefined;

      this.emit(PJSEvent.Disconnected, {});
      
      if (this.config.debug) {
        console.log('[PJS] Disconnected from server');
      }
    } catch (error) {
      const pjsError = new PJSError(
        PJSErrorType.ConnectionError,
        'Error during disconnect',
        undefined,
        error as Error
      );
      
      this.emit(PJSEvent.Error, { error: pjsError, context: 'disconnect' });
      throw pjsError;
    }
  }

  /**
   * Stream JSON data from server endpoint
   * 
   * @param endpoint - Server endpoint to stream from
   * @param options - Streaming options
   * @returns Promise resolving to complete JSON data
   */
  async stream<T = any>(endpoint: string, options: StreamOptions = {}): Promise<T> {
    if (!this.isConnected) {
      await this.connect();
    }

    const streamId = this.generateStreamId();
    this.currentStreamId = streamId;

    // Initialize stream statistics
    const streamStats: StreamStats = {
      streamId,
      startTime: Date.now(),
      totalFrames: 0,
      priorityDistribution: {},
      performance: {
        timeToFirstFrame: 0,
        timeToSkeleton: 0,
        timeToCompletion: 0,
        throughputMbps: 0,
        framesPerSecond: 0,
        memoryStats: {
          totalAllocated: 0,
          totalReferenced: 0,
          efficiency: 0,
          peakUsage: 0
        }
      }
    };
    
    this.streams.set(streamId, streamStats);

    try {
      return await this.processStream<T>(endpoint, streamId, options);
    } catch (error) {
      const pjsError = error instanceof PJSError 
        ? error 
        : new PJSError(
            PJSErrorType.ProtocolError,
            `Stream failed for endpoint: ${endpoint}`,
            { endpoint, streamId },
            error as Error
          );
      
      this.emit(PJSEvent.Error, { error: pjsError, context: 'stream' });
      throw pjsError;
    } finally {
      // Cleanup
      this.currentStreamId = undefined;
      streamStats.endTime = Date.now();
    }
  }

  /**
   * Get statistics for all streams
   */
  getStreamStats(): StreamStats[] {
    return Array.from(this.streams.values());
  }

  /**
   * Get current session ID
   */
  getSessionId(): string | undefined {
    return this.sessionId;
  }

  /**
   * Check if client is connected
   */
  isClientConnected(): boolean {
    return this.isConnected;
  }

  // Event listener helpers with type safety
  
  on<K extends keyof PJSEventData>(
    event: K, 
    listener: EventListener<PJSEventData[K]>
  ): this {
    return super.on(event, listener);
  }

  emit<K extends keyof PJSEventData>(
    event: K, 
    data: PJSEventData[K]
  ): boolean {
    return super.emit(event, data);
  }

  // Private methods

  private validateAndNormalizeConfig(config: PJSClientConfig): Required<PJSClientConfig> {
    if (!config.baseUrl) {
      throw new PJSError(
        PJSErrorType.ConfigurationError,
        'baseUrl is required in client configuration'
      );
    }

    return {
      baseUrl: config.baseUrl.replace(/\/$/, ''), // Remove trailing slash
      transport: config.transport ?? TransportType.HTTP,
      sessionId: config.sessionId,
      headers: config.headers ?? {},
      timeout: config.timeout ?? 30000,
      debug: config.debug ?? false,
      bufferSize: config.bufferSize ?? 1024 * 1024, // 1MB
      priorityThreshold: config.priorityThreshold ?? Priority.Background,
      maxConcurrentStreams: config.maxConcurrentStreams ?? 10
    };
  }

  private createTransport(): Transport {
    switch (this.config.transport) {
      case TransportType.HTTP:
        return new HttpTransport(this.config);
      case TransportType.WebSocket:
        return new WebSocketTransport(this.config);
      case TransportType.ServerSentEvents:
        return new SSETransport(this.config);
      default:
        throw new PJSError(
          PJSErrorType.ConfigurationError,
          `Unsupported transport type: ${this.config.transport}`
        );
    }
  }

  private setupEventHandlers(): void {
    this.transport.on('frame', (frame: Frame) => {
      this.handleFrame(frame);
    });

    this.transport.on('error', (error: Error) => {
      this.emit(PJSEvent.Error, { 
        error: new PJSError(
          PJSErrorType.ConnectionError,
          'Transport error',
          undefined,
          error
        ), 
        context: 'transport' 
      });
    });

    this.transport.on('disconnect', () => {
      this.isConnected = false;
      this.emit(PJSEvent.Disconnected, { reason: 'Transport disconnected' });
    });
  }

  private async processStream<T>(
    endpoint: string, 
    streamId: string, 
    options: StreamOptions
  ): Promise<T> {
    const stats = this.streams.get(streamId)!;
    let result: T | undefined;
    let skeletonReceived = false;
    let progressInfo: ProgressInfo = {
      framesReceived: 0,
      bytesReceived: 0,
      elapsedTime: 0,
      prioritiesReceived: []
    };

    return new Promise<T>((resolve, reject) => {
      // Set up frame handler for this stream
      const frameHandler = (frame: Frame) => {
        if (this.currentStreamId !== streamId) return; // Ignore frames for other streams

        try {
          stats.totalFrames++;
          progressInfo.framesReceived++;
          progressInfo.elapsedTime = Date.now() - stats.startTime;
          
          // Update priority distribution
          if (!stats.priorityDistribution[frame.priority]) {
            stats.priorityDistribution[frame.priority] = 0;
          }
          stats.priorityDistribution[frame.priority]++;

          // Track unique priorities received
          if (!progressInfo.prioritiesReceived.includes(frame.priority)) {
            progressInfo.prioritiesReceived.push(frame.priority);
          }

          if (frame.type === FrameType.Skeleton) {
            if (!skeletonReceived) {
              stats.performance.timeToFirstFrame = progressInfo.elapsedTime;
              stats.performance.timeToSkeleton = progressInfo.elapsedTime;
              skeletonReceived = true;
            }

            result = this.jsonReconstructor.applySkeleton(frame.data);
            
            this.emit(PJSEvent.SkeletonReady, {
              data: result,
              processingTime: progressInfo.elapsedTime
            });

            // Call render callback if provided
            if (options.onRender) {
              options.onRender(result, {
                priority: frame.priority,
                isComplete: false,
                progress: progressInfo
              });
            }

          } else if (frame.type === FrameType.Patch) {
            if (!result) {
              throw new PJSError(
                PJSErrorType.ProtocolError,
                'Received patch frame before skeleton'
              );
            }

            for (const patch of frame.patches) {
              result = this.jsonReconstructor.applyPatch(result, patch);
              
              this.emit(PJSEvent.PatchApplied, {
                patch,
                path: patch.path,
                priority: frame.priority,
                resultingData: result
              });
            }

            // Call render callback for patch updates
            if (options.onRender) {
              options.onRender(result, {
                priority: frame.priority,
                isComplete: false,
                progress: progressInfo
              });
            }

          } else if (frame.type === FrameType.Complete) {
            stats.performance.timeToCompletion = progressInfo.elapsedTime;
            
            // Calculate final performance metrics
            const totalTime = progressInfo.elapsedTime / 1000; // seconds
            stats.performance.framesPerSecond = stats.totalFrames / totalTime;
            
            if (frame.total_frames) {
              progressInfo.totalFrames = frame.total_frames;
              progressInfo.completionPercentage = 100;
            }

            this.emit(PJSEvent.StreamComplete, {
              data: result!,
              stats: progressInfo,
              totalTime: progressInfo.elapsedTime
            });

            // Final render callback
            if (options.onRender) {
              options.onRender(result!, {
                priority: frame.priority,
                isComplete: true,
                progress: progressInfo
              });
            }

            resolve(result!);
            return;
          }

          // Emit progress update
          this.emit(PJSEvent.ProgressUpdate, progressInfo);
          
          if (options.onProgress) {
            options.onProgress(progressInfo);
          }

        } catch (error) {
          reject(error instanceof PJSError ? error : new PJSError(
            PJSErrorType.ProtocolError,
            'Error processing frame',
            { frame, streamId },
            error as Error
          ));
        }
      };

      // Set up timeout
      const timeout = setTimeout(() => {
        reject(new PJSError(
          PJSErrorType.TimeoutError,
          `Stream timeout after ${options.timeout || this.config.timeout}ms`,
          { endpoint, streamId }
        ));
      }, options.timeout || this.config.timeout);

      // Start listening for frames
      this.transport.on('frame', frameHandler);

      // Start the stream
      this.transport.startStream(endpoint, {
        sessionId: this.sessionId!,
        streamId,
        queryParams: options.queryParams,
        headers: options.headers
      }).then(() => {
        if (this.config.debug) {
          console.log(`[PJS] Started stream ${streamId} for endpoint: ${endpoint}`);
        }
      }).catch((error) => {
        clearTimeout(timeout);
        reject(error);
      });

      // Cleanup function
      const cleanup = () => {
        clearTimeout(timeout);
        this.transport.removeListener('frame', frameHandler);
      };

      // Ensure cleanup happens
      const originalResolve = resolve;
      const originalReject = reject;
      
      resolve = (value: T) => {
        cleanup();
        originalResolve(value);
      };
      
      reject = (reason?: any) => {
        cleanup();
        originalReject(reason);
      };
    });
  }

  private handleFrame(frame: Frame): void {
    this.emit(PJSEvent.FrameReceived, { frame });
    
    if (this.config.debug) {
      console.log('[PJS] Received frame:', {
        type: frame.type,
        priority: frame.priority,
        timestamp: frame.timestamp
      });
    }
  }

  private generateStreamId(): string {
    return `stream_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
  }
}