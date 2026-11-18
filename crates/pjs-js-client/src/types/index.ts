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
 * Frame types in PJS protocol.
 *
 * Values match the wire representation of the Rust `FrameType` enum's
 * derived `Serialize` impl (`crates/pjs-domain/src/entities/frame.rs`) —
 * a bare capitalized variant name, since that enum has no `rename_all`.
 * The HTTP transport (`transport/http.ts`) parses these values directly
 * off the REST streaming routes.
 */
export enum FrameType {
  Skeleton = 'Skeleton',
  Patch = 'Patch',
  Complete = 'Complete',
  Error = 'Error'
}

/**
 * Base frame structure
 */
export interface BaseFrame {
  /** Field name matches the Rust `Frame`'s `frame_type` field (was `type`). */
  frame_type: FrameType;
  priority: Priority;
  /** Present on frames deserialized from the REST routes; absent on WASM-native frames. */
  stream_id?: string;
  /** Present on frames deserialized from the REST routes; absent on WASM-native frames. */
  sequence?: number;
  timestamp?: number;
  metadata?: Record<string, unknown>;
}

/**
 * Skeleton frame - initial structure with empty/minimal values.
 *
 * `payload` carries the raw skeleton JSON structure directly, matching the
 * Rust `Frame`'s `payload` field for a skeleton frame (`Frame::skeleton`'s
 * `skeleton_data` argument, stored verbatim, not wrapped further).
 */
export interface SkeletonFrame extends BaseFrame {
  frame_type: FrameType.Skeleton;
  payload: any; // JSON skeleton structure
  complete: false;
}

/**
 * Any JSON value.
 */
export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

/**
 * Wire patch operations, in canonical order.
 */
export const PATCH_OPERATIONS = ['set', 'append', 'merge', 'delete', 'append_str'] as const;

/**
 * Wire patch operation kind derived from {@link PATCH_OPERATIONS}.
 */
export type PatchOperationKind = (typeof PATCH_OPERATIONS)[number];

/**
 * Type guard for {@link PATCH_OPERATIONS}: narrows `value` to a wire patch
 * operation, so validators share one source of truth with the type.
 */
export function isPatchOperationKind(value: unknown): value is PatchOperationKind {
  return PATCH_OPERATIONS.some((operation) => operation === value);
}

/**
 * Patch operation for updating JSON structure
 */
export interface PatchOperation {
  path: JsonPath;
  value: any;
  operation: PatchOperationKind;
}

/**
 * Patch frame - incremental updates to JSON structure.
 *
 * `patches` lives under `payload`, matching the Rust `Frame`'s wire shape
 * for a patch frame (`Frame::patch` builds `payload: {"patches": [...]}`),
 * not as a top-level field.
 */
export interface PatchFrame extends BaseFrame {
  frame_type: FrameType.Patch;
  payload: {
    patches: PatchOperation[];
  };
}

/**
 * Complete frame - signals end of streaming.
 *
 * `checksum` lives under `payload`, matching the Rust `Frame`'s wire shape
 * for a complete frame (`Frame::complete` builds `payload: {"checksum": ...}`
 * or `payload: {}` when no checksum was supplied). `payload` itself is
 * optional here only because WASM-native complete frames (`wasm-backend.ts`,
 * `wasm-parser.ts`) carry no payload at all — a REST-sourced complete frame
 * always has one, even if empty.
 */
export interface CompleteFrame extends BaseFrame {
  frame_type: FrameType.Complete;
  payload?: {
    checksum?: string;
  };
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

  /**
   * Maximum cumulative UTF-8 bytes of `StreamPatch` messages accepted for one
   * WebSocket stream; a stream that exceeds it is dropped with a
   * `ProtocolError`.
   * @default 16 MiB
   */
  maxStreamBytes?: number;

  /**
   * How the per-frame state reaches `onRender` and `PatchApplied` listeners.
   * `'live'` hands over the reconstructor's own state without a copy: treat it
   * as read-only and do not keep it across frames. `'snapshot'` hands over an
   * independent deep clone for every frame. The value `stream()` resolves with
   * is always an independent clone.
   * @default 'live'
   */
  stateDelivery?: StateDelivery;
}

/** Selects how per-frame state is delivered, see {@link PJSClientConfig.stateDelivery}. */
export type StateDelivery = 'live' | 'snapshot';

/** Default for {@link PJSClientConfig.maxStreamBytes}. */
export const DEFAULT_MAX_STREAM_BYTES = 16 * 1024 * 1024;

/** Most path segments a `StreamPatch` patch may address (spec 067). */
export const MAX_PATCH_PATH_DEPTH = 128;

/** A value deeply marked read-only, the type of the `'live'` per-frame state. */
export type DeepReadonly<T> = T extends readonly (infer U)[]
  ? readonly DeepReadonly<U>[]
  : T extends object
    ? { readonly [K in keyof T]: DeepReadonly<T[K]> }
    : T;

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
  ProgressUpdate = 'progress_update',
  ServerShutdown = 'server_shutdown'
}

/**
 * Progress information for streaming
 */
export interface ProgressInfo {
  framesReceived: number;
  totalFrames?: number;
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
  [PJSEvent.SkeletonReady]: { data: DeepReadonly<JsonValue> | undefined; processingTime: number };
  [PJSEvent.PatchApplied]: { 
    patch: PatchOperation; 
    path: JsonPath; 
    priority: Priority;
    resultingData?: DeepReadonly<JsonValue>;
    appliedPatches: number;
    skippedPatches: number;
  };
  [PJSEvent.StreamComplete]: { 
    data: any; 
    stats: ProgressInfo;
    totalTime: number;
  };
  [PJSEvent.ProgressUpdate]: ProgressInfo;
  [PJSEvent.ServerShutdown]: Record<string, never>;
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
  priorityDistribution: Partial<Record<Priority, number>>;
  performance: PerformanceMetrics;
}

// Error Types

/**
 * PJS-specific error types
 */
export enum PJSErrorType {
  ConnectionError = 'CONNECTION_ERROR',
  ProtocolError = 'PROTOCOL_ERROR',
  ProtocolViolation = 'PROTOCOL_VIOLATION',
  ParseError = 'PARSE_ERROR',
  ValidationError = 'VALIDATION_ERROR',
  TimeoutError = 'TIMEOUT_ERROR',
  ConfigurationError = 'CONFIGURATION_ERROR',
  InitializationError = 'INITIALIZATION_ERROR',
  StreamError = 'STREAM_ERROR',
  ServerShutdown = 'SERVER_SHUTDOWN'
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
export type RenderCallback = (data: DeepReadonly<JsonValue> | undefined, metadata: {
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
   * JSON document to stream (required by the WebSocket transport).
   */
  data?: JsonValue;

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