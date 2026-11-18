/**
 * WebSocket Transport - speaks the shipped pjs-core `WsMessage` protocol on `/ws`.
 *
 * One stream at a time: the client sends `StreamInit` with the document, the
 * server answers with `StreamPatch` messages and a final `StreamComplete`,
 * which this transport maps to Skeleton, Patch and Complete frames. There is
 * no automatic reconnection.
 */

import { ConnectResult, StreamOptions, Transport } from './base.js';
import { isRecord } from './json-record.js';
import { formatJsonPath, JsonPathSegment } from '../core/json-path.js';
import {
  Frame,
  FrameType,
  isPatchOperationKind,
  MAX_PATCH_PATH_DEPTH,
  PatchOperation,
  PJSError,
  PJSErrorType,
  Priority
} from '../types/index.js';

/** Frame size the client asks the server to split the document by. */
const MAX_FRAME_SIZE = 64 * 1024;

/** UTF-8 byte length of a string without encoding it; a lone surrogate counts as U+FFFD (3 bytes), as in `TextEncoder`. */
function utf8ByteLength(text: string): number {
  let bytes = 0;
  for (let i = 0; i < text.length; i++) {
    const unit = text.charCodeAt(i);
    if (unit < 0x80) {
      bytes += 1;
    } else if (unit < 0x800) {
      bytes += 2;
    } else if (unit >= 0xd800 && unit < 0xdc00 && i + 1 < text.length && (text.charCodeAt(i + 1) & 0xfc00) === 0xdc00) {
      bytes += 4;
      i++;
    } else {
      bytes += 3;
    }
  }
  return bytes;
}

/** Close code for a deliberate client disconnect. */
const CLOSE_NORMAL = 1000;

/** A stream the transport is currently driving. */
interface ActiveStream {
  readonly sessionId: string;
  nextFrameId: number;
  patchFrames: number;
  patchBytes: number;
  skeletonSent: boolean;
  readonly resolve: () => void;
  readonly reject: (error: PJSError) => void;
}

interface ParsedPatchMessage {
  readonly frameId: number;
  readonly priority: number;
  readonly patches: PatchOperation[];
}

function isIndex(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

function parseSegments(value: unknown): JsonPathSegment[] | null {
  if (!Array.isArray(value) || value.length > MAX_PATCH_PATH_DEPTH) return null;
  const segments: JsonPathSegment[] = [];
  for (const segment of value) {
    if (typeof segment === 'string') {
      segments.push({ kind: 'key', key: segment });
    } else if (isIndex(segment)) {
      segments.push({ kind: 'index', index: segment });
    } else {
      return null;
    }
  }
  return segments;
}

/** Validates a `StreamPatch` payload; returns an explanation instead of throwing. */
function parsePatchMessage(data: Record<string, unknown>): ParsedPatchMessage | string {
  const { frame_id: frameId, priority, patches } = data;
  if (!isIndex(frameId)) return 'StreamPatch frame_id must be a non-negative integer';
  if (typeof priority !== 'number' || !Number.isFinite(priority)) {
    return 'StreamPatch priority must be a number';
  }
  if (!Array.isArray(patches)) return 'StreamPatch patches must be an array';

  const operations: PatchOperation[] = [];
  for (const entry of patches as unknown[]) {
    if (!Array.isArray(entry) || entry.length !== 3) {
      return 'StreamPatch patch must be [operation, path, value]';
    }
    const [operation, rawSegments, value] = entry as [unknown, unknown, unknown];
    const segments = parseSegments(rawSegments);
    if (!isPatchOperationKind(operation)) return `StreamPatch has unknown operation: ${String(operation)}`;
    if (segments === null) {
      return `StreamPatch path must be at most ${MAX_PATCH_PATH_DEPTH} strings or non-negative integers`;
    }
    operations.push({ operation, path: formatJsonPath(segments), value });
  }
  return { frameId, priority, patches: operations };
}

/**
 * WebSocket transport implementation.
 *
 * Requires `globalThis.WebSocket` (browsers, Node 22+, or a polyfill such as
 * `ws`) and `globalThis.crypto.randomUUID`.
 */
export class WebSocketTransport extends Transport {
  private ws?: WebSocket;
  private active?: ActiveStream;
  private rejectPendingConnect?: (error: PJSError) => void;

  async connect(): Promise<ConnectResult> {
    this.dropSocket('Superseded by a new connect()');
    return new Promise((resolve, reject) => {
      const wsUrl = `${this.config.baseUrl.replace(/^http/, 'ws').replace(/\/$/, '')}/ws`;
      let socket: WebSocket;
      try {
        socket = new WebSocket(wsUrl);
      } catch (error) {
        reject(new PJSError(
          PJSErrorType.ConnectionError,
          'Failed to create WebSocket connection',
          undefined,
          error as Error
        ));
        return;
      }
      this.ws = socket;
      this.rejectPendingConnect = reject;

      socket.onopen = () => {
        if (this.ws !== socket) return;
        this.rejectPendingConnect = undefined;
        this.isConnected = true;
        if (this.config.debug) {
          console.log('[PJS WebSocket] Connected to:', wsUrl);
        }
        resolve({ sessionId: undefined, supportedFeatures: ['patch-framing'] });
      };

      socket.onmessage = (event) => {
        if (this.ws !== socket) return;
        this.handleMessage(event.data);
      };

      socket.onerror = (error) => {
        if (this.ws !== socket) return;
        const pjsError = new PJSError(
          PJSErrorType.ConnectionError,
          'WebSocket connection error',
          { error }
        );
        if (!this.isConnected) {
          this.rejectPendingConnect = undefined;
          reject(pjsError);
        } else {
          this.emitError(pjsError);
        }
      };

      socket.onclose = () => {
        if (this.ws !== socket) return;
        const wasConnected = this.isConnected;
        this.rejectPendingConnect = undefined;
        this.isConnected = false;
        this.ws = undefined;
        const closed = new PJSError(PJSErrorType.ConnectionError, 'WebSocket closed');
        this.settleActive((stream) => stream.reject(closed));
        if (wasConnected) {
          this.emitDisconnect();
        } else {
          reject(closed);
        }
      };
    });
  }

  async disconnect(): Promise<void> {
    this.dropSocket('Client disconnected');
  }

  /**
   * Closes the current socket and rejects whatever waits on it: a pending
   * `connect()` and the active stream. Settling happens before `this.ws` is
   * cleared so the stale-socket guard cannot swallow the rejection.
   */
  private dropSocket(reason: string): void {
    const socket = this.ws;
    const sessionId = this.active?.sessionId;
    this.settleActive((stream) => stream.reject(
      new PJSError(PJSErrorType.ConnectionError, reason, { sessionId: stream.sessionId })
    ));
    const rejectConnect = this.rejectPendingConnect;
    this.rejectPendingConnect = undefined;
    rejectConnect?.(new PJSError(PJSErrorType.ConnectionError, reason, { sessionId }));
    this.ws = undefined;
    this.isConnected = false;
    socket?.close(CLOSE_NORMAL, 'Client disconnect');
  }

  /**
   * Streams `options.data` through the server. `endpoint` is ignored: the
   * WebSocket route is fixed. Resolves on `StreamComplete`.
   */
  async startStream(_endpoint: string, options: StreamOptions): Promise<void> {
    const socket = this.ws;
    if (!socket || !this.isConnected) {
      throw new PJSError(PJSErrorType.ConnectionError, 'WebSocket not connected');
    }
    if (options.data === undefined) {
      throw new PJSError(
        PJSErrorType.ConfigurationError,
        'The WebSocket transport requires StreamOptions.data (the document to stream)'
      );
    }
    const randomUuid = (globalThis as { crypto?: { randomUUID?: () => string } }).crypto?.randomUUID;
    if (typeof randomUuid !== 'function') {
      throw new PJSError(
        PJSErrorType.ConfigurationError,
        'The WebSocket transport requires crypto.randomUUID'
      );
    }

    const sessionId = randomUuid.call(globalThis.crypto);
    // Serialize before touching the active stream: a throw here must leave the previous stream untouched.
    const message = JSON.stringify({
      type: 'StreamInit',
      data: {
        session_id: sessionId,
        data: options.data,
        options: { max_frame_size: MAX_FRAME_SIZE, framing: 'patch' }
      }
    });

    this.endActive('Stream superseded');
    socket.send(message);

    // Register only after a successful send, with no await in between, so a failure leaves no orphaned stream.
    return new Promise<void>((resolve, reject) => {
      this.active = {
        sessionId,
        nextFrameId: 0,
        patchFrames: 0,
        patchBytes: 0,
        skeletonSent: false,
        resolve,
        reject
      };
    });
  }

  async stopStream(): Promise<void> {
    this.endActive('Stream stopped');
  }

  /** Cancels the active stream on the server and rejects it with a `StreamError`; no-op without one. */
  private endActive(reason: string): void {
    const stream = this.active;
    if (!stream) return;
    this.cancel(stream.sessionId);
    this.settleActive((active) => active.reject(
      new PJSError(PJSErrorType.StreamError, reason, { sessionId: active.sessionId })
    ));
  }

  private cancel(sessionId: string): void {
    this.ws?.send(JSON.stringify({ type: 'StreamCancel', data: { session_id: sessionId } }));
  }

  private settleActive(settle: (stream: ActiveStream) => void): void {
    const stream = this.active;
    this.active = undefined;
    if (stream) settle(stream);
  }

  /** Fails the active stream because the server broke the protocol, and tells the server to stop. */
  private failProtocol(stream: ActiveStream, message: string, context?: Record<string, unknown>): void {
    this.cancel(stream.sessionId);
    this.settleActive((active) => active.reject(new PJSError(PJSErrorType.ProtocolError, message, context)));
  }

  private handleMessage(raw: unknown): void {
    if (typeof raw !== 'string') return;
    let message: unknown;
    try {
      message = JSON.parse(raw);
    } catch (error) {
      this.emitError(new PJSError(
        PJSErrorType.ParseError,
        'Failed to parse WebSocket message',
        { data: raw },
        error as Error
      ));
      return;
    }
    if (!isRecord(message) || !isRecord(message.data)) return;

    const stream = this.active;
    if (!stream) return;
    const { data } = message;
    const sessionId = data.session_id;

    switch (message.type) {
      case 'StreamPatch':
        if (sessionId === stream.sessionId) this.handleStreamPatch(stream, data, raw);
        break;
      case 'StreamComplete':
        if (sessionId === stream.sessionId) this.handleStreamComplete(stream);
        break;
      case 'Error':
        if (sessionId === null || sessionId === stream.sessionId) this.handleServerError(data);
        break;
      default:
        if (this.config.debug) {
          console.log('[PJS WebSocket] Ignoring message type:', message.type);
        }
    }
  }

  private handleStreamPatch(stream: ActiveStream, data: Record<string, unknown>, raw: string): void {
    stream.patchBytes += utf8ByteLength(raw);
    const limit = this.config.maxStreamBytes;
    if (stream.patchBytes > limit) {
      this.failProtocol(stream, `Stream exceeded maxStreamBytes (${limit})`, { sessionId: stream.sessionId });
      return;
    }
    const parsed = parsePatchMessage(data);
    if (typeof parsed === 'string') {
      this.failProtocol(stream, parsed);
      return;
    }
    if (parsed.frameId !== stream.nextFrameId) {
      this.failProtocol(
        stream,
        `StreamPatch frame_id ${parsed.frameId} out of order (expected ${stream.nextFrameId})`
      );
      return;
    }
    stream.nextFrameId++;
    if (parsed.patches.length === 0) return;

    this.emitSkeletonOnce(stream);
    // A listener may have stopped, superseded or disconnected this stream while the skeleton was emitted.
    if (this.active !== stream) return;
    stream.patchFrames++;
    this.emitGuarded(stream, {
      frame_type: FrameType.Patch,
      priority: parsed.priority,
      sequence: parsed.frameId,
      stream_id: stream.sessionId,
      payload: { patches: parsed.patches }
    });
  }

  /** Emits a frame; a throwing listener fails the stream instead of escaping `onmessage`. */
  private emitGuarded(stream: ActiveStream, frame: Frame): void {
    try {
      this.emitFrame(frame);
    } catch (error) {
      // A listener that already stopped or superseded the stream has settled it; nothing is left to fail.
      if (this.active !== stream) return;
      this.cancel(stream.sessionId);
      this.settleActive((active) => active.reject(new PJSError(
        PJSErrorType.StreamError,
        'Frame listener threw',
        { sessionId: active.sessionId },
        error as Error
      )));
    }
  }

  private handleStreamComplete(stream: ActiveStream): void {
    this.emitSkeletonOnce(stream);
    if (this.active !== stream) return;
    // total_frames counts the skeleton, the patch frames and this Complete frame.
    // Detach before emitting: a listener may start the next stream re-entrantly, and a completed stream needs no StreamCancel.
    this.active = undefined;
    try {
      this.emitFrame({
        frame_type: FrameType.Complete,
        priority: Priority.Critical,
        stream_id: stream.sessionId,
        total_frames: stream.patchFrames + 2
      });
    } finally {
      stream.resolve();
    }
  }

  private handleServerError(data: Record<string, unknown>): void {
    const { code, error } = data;
    if (typeof code !== 'number' || typeof error !== 'string') {
      const stream = this.active;
      if (stream) this.failProtocol(stream, 'Malformed Error reply', { sessionId: stream.sessionId });
      return;
    }
    this.settleActive((stream) =>
      stream.reject(new PJSError(PJSErrorType.ProtocolError, error, { code, error }))
    );
  }

  /** The server sends no skeleton: the client starts from an empty document and the first patch fills it. */
  private emitSkeletonOnce(stream: ActiveStream): void {
    if (stream.skeletonSent) return;
    stream.skeletonSent = true;
    this.emitGuarded(stream, {
      frame_type: FrameType.Skeleton,
      priority: Priority.Critical,
      payload: {},
      complete: false,
      stream_id: stream.sessionId
    });
  }
}
