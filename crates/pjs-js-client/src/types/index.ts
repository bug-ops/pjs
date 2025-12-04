/**
 * Priority JSON Streaming Protocol (PJS) TypeScript Types
 * 
 * This module defines the core types for the PJS protocol,
 * matching the Rust implementation for full compatibility.
 */

// Core PJS Protocol Types

/**
 * Priority levels for JSON fields and frames
 */
export enum Priority {
  Critical = 100,
  High = 75,
  Medium = 50,
  Low = 25,
  Background = 10
}

/**
 * JSON Path for addressing specific nodes in JSON structure
 * Format: $.root.field[0].subfield
 */
export type JsonPath = string;

/**
 * Frame types in PJS protocol
 */
export enum FrameType {
  Skeleton = 'skeleton',
  Patch = 'patch', 
  Complete = 'complete'
}

/**
 * Base frame structure
 */
export interface BaseFrame {
  type: FrameType;
  priority: Priority;
  timestamp?: number;
  metadata?: Record<string, unknown>;
}

/**
 * Skeleton frame - initial structure with empty/minimal values
 */
export interface SkeletonFrame extends BaseFrame {
  type: FrameType.Skeleton;
  data: any; // JSON skeleton structure
  complete: false;
}

/**
 * Patch operation for updating JSON structure
 */
export interface PatchOperation {
  path: JsonPath;
  value: any;
  operation: 'set' | 'append' | 'merge' | 'delete';
}

/**
 * Patch frame - incremental updates to JSON structure
 */
export interface PatchFrame extends BaseFrame {
  type: FrameType.Patch;
  patches: PatchOperation[];
}

/**
 * Complete frame - signals end of streaming
 */
export interface CompleteFrame extends BaseFrame {
  type: FrameType.Complete;
  checksum?: string;
  total_frames?: number;
}

/**
 * Union type for all frame types
 */
export type Frame = SkeletonFrame | PatchFrame | CompleteFrame;

// Client Configuration Types

/**
 * Transport protocol options
 */
export enum TransportType {
  HTTP = 'http',
  WebSocket = 'websocket',
  ServerSentEvents = 'sse',
  WASM = 'wasm'
}

/**
 * Client configuration options
 */
export interface PJSClientConfig {
  /**
   * Base URL for PJS server
   */
  baseUrl: string;

  /**
   * Transport protocol to use
   * @default TransportType.HTTP
   */
  transport?: TransportType;

  /**
   * Session ID for existing session (optional)
   */
  sessionId?: string;

  /**
   * Custom headers for requests
   */
  headers?: Record<string, string>;

  /**
   * Request timeout in milliseconds
   * @default 30000
   */
  timeout?: number;

  /**
   * Enable debug logging
   * @default false
   */
  debug?: boolean;

  /**
   * Buffer size for reconstruction
   * @default 1024 * 1024 (1MB)
   */
  bufferSize?: number;

  /**
   * Priority threshold - only process frames with priority >= this value
   * @default Priority.Background
   */
  priorityThreshold?: Priority;

  /**
   * Maximum number of concurrent streams
   * @default 10
   */
  maxConcurrentStreams?: number;
}

// Event System Types

/**
 * Events emitted by PJS client
 */
export enum PJSEvent {
  Connected = 'connected',
  Disconnected = 'disconnected',
  Error = 'error',
  FrameReceived = 'frame_received',
  SkeletonReady = 'skeleton_ready',
  PatchApplied = 'patch_applied',
  StreamComplete = 'stream_complete',
  ProgressUpdate = 'progress_update'
}

/**
 * Progress information for streaming
 */
export interface ProgressInfo {
  framesReceived: number;
  totalFrames?: number;
  bytesReceived: number;
  elapsedTime: number;
  prioritiesReceived: Priority[];
  completionPercentage?: number;
}

/**
 * Event data structures
 */
export interface PJSEventData {
  [PJSEvent.Connected]: { sessionId: string };
  [PJSEvent.Disconnected]: { reason?: string };
  [PJSEvent.Error]: { error: Error; context?: string };
  [PJSEvent.FrameReceived]: { frame: Frame };
  [PJSEvent.SkeletonReady]: { data: any; processingTime: number };
  [PJSEvent.PatchApplied]: { 
    patch: PatchOperation; 
    path: JsonPath; 
    priority: Priority;
    resultingData?: any;
  };
  [PJSEvent.StreamComplete]: { 
    data: any; 
    stats: ProgressInfo;
    totalTime: number;
  };
  [PJSEvent.ProgressUpdate]: ProgressInfo;
}

// Utility Types

/**
 * Event listener function type
 */
export type EventListener<T = any> = (data: T) => void | Promise<void>;

/**
 * Memory usage statistics
 */
export interface MemoryStats {
  totalAllocated: number;
  totalReferenced: number;
  efficiency: number; // percentage
  peakUsage: number;
}

/**
 * Performance metrics
 */
export interface PerformanceMetrics {
  timeToFirstFrame: number;
  timeToSkeleton: number;
  timeToCompletion: number;
  throughputMbps: number;
  framesPerSecond: number;
  memoryStats: MemoryStats;
}

/**
 * Stream statistics
 */
export interface StreamStats {
  streamId: string;
  startTime: number;
  endTime?: number;
  totalFrames: number;
  priorityDistribution: Record<Priority, number>;
  performance: PerformanceMetrics;
}

// Error Types

/**
 * PJS-specific error types
 */
export enum PJSErrorType {
  ConnectionError = 'CONNECTION_ERROR',
  ProtocolError = 'PROTOCOL_ERROR', 
  ParseError = 'PARSE_ERROR',
  ValidationError = 'VALIDATION_ERROR',
  TimeoutError = 'TIMEOUT_ERROR',
  ConfigurationError = 'CONFIGURATION_ERROR'
}

/**
 * PJS error with additional context
 */
export class PJSError extends Error {
  constructor(
    public type: PJSErrorType,
    message: string,
    public context?: any,
    public originalError?: Error
  ) {
    super(message);
    this.name = 'PJSError';
  }
}

// Advanced Types

/**
 * Priority strategy for custom prioritization
 */
export interface PriorityStrategy {
  name: string;
  calculatePriority(path: JsonPath, value: any, context?: any): Priority;
}

/**
 * Render callback for progressive UI updates
 */
export type RenderCallback = (data: any, metadata: {
  priority: Priority;
  path?: JsonPath;
  isComplete: boolean;
  progress: ProgressInfo;
}) => void | Promise<void>;

/**
 * Stream options for individual requests
 */
export interface StreamOptions {
  /**
   * Custom priority strategy
   */
  priorityStrategy?: PriorityStrategy;

  /**
   * Render callback for progressive updates
   */
  onRender?: RenderCallback;

  /**
   * Progress callback
   */
  onProgress?: EventListener<ProgressInfo>;

  /**
   * Custom timeout for this stream
   */
  timeout?: number;

  /**
   * Additional query parameters
   */
  queryParams?: Record<string, string>;

  /**
   * Custom request headers
   */
  headers?: Record<string, string>;
}