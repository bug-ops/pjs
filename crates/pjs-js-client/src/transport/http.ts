/**
 * HTTP Transport - speaks the shipped pjs-core REST routes.
 *
 * One stream at a time: `POST /pjs/sessions/{id}/streams` with the document,
 * `POST .../start`, `POST .../generate-frames` (which fills the frame store),
 * then `GET .../frames/stream` read as NDJSON. The server serves patch frames
 * only, so the transport synthesizes the Skeleton and Complete frames the
 * client expects. There is no automatic reconnection.
 */

import { field } from './json-record.js';
import { ConnectResult, isShutdownMarker, StreamOptions, Transport } from './base.js';
import { CompleteFrame, Frame, FrameType, JsonValue, PJSError, PJSErrorType, Priority } from '../types/index.js';

const CLIENT_INFO = 'pjs-js-client';

/** Largest frame page the server serves (`MAX_FRAMES_PER_REQUEST`). */
const MAX_FRAMES = 1000;

function frameTypeOf(value: unknown): unknown {
  return field(value, 'frame_type');
}

/**
 * Counts the frames of one stream and fills in what the server page lacks.
 *
 * The REST page holds patch frames only. A Skeleton frame is emitted before
 * the first non-skeleton frame and a Complete frame at {@link finish}, unless
 * the server sent them. The Complete frame carries `total_frames` = skeleton
 * + patches + complete, which is what `PJSClient` reports as `totalFrames`.
 */
export class FrameLedger {
  private emitted = 0;
  private skeleton = false;
  private complete = false;
  private serverFrames = 0;

  constructor(
    private readonly streamId: string,
    private readonly emit: (frame: Frame) => void
  ) {}

  /** Number of frames pushed so far, i.e. what the server delivered. */
  get received(): number {
    return this.serverFrames;
  }

  /** True once a Complete frame, sent or synthesized, was emitted. */
  get completed(): boolean {
    return this.complete;
  }

  /** Emits one server frame; frames of an unknown shape pass through for the client to reject. */
  push(frame: unknown): void {
    const type = frameTypeOf(frame);
    if (type !== FrameType.Skeleton && type !== FrameType.Patch && type !== FrameType.Complete) {
      this.emit(frame as Frame); // validated by PJSClient before use
      return;
    }
    this.serverFrames++;
    if (type === FrameType.Skeleton) {
      this.skeleton = true;
    } else {
      this.emitSkeletonOnce();
    }
    if (type === FrameType.Complete) {
      this.complete = true;
      this.emitCounted({ ...(frame as CompleteFrame), total_frames: this.emitted + 1 });
      return;
    }
    this.emitCounted(frame as Frame); // a Skeleton or Patch frame; PJSClient validates the rest
  }

  /** Emits the Skeleton and Complete frames the server did not send. */
  finish(): void {
    this.emitSkeletonOnce();
    if (this.complete) return;
    this.complete = true;
    this.emitCounted({
      frame_type: FrameType.Complete,
      priority: Priority.Critical,
      stream_id: this.streamId,
      total_frames: this.emitted + 1
    });
  }

  private emitSkeletonOnce(): void {
    if (this.skeleton) return;
    this.skeleton = true;
    this.emitCounted({
      frame_type: FrameType.Skeleton,
      priority: Priority.Critical,
      payload: {},
      complete: false,
      stream_id: this.streamId
    });
  }

  private emitCounted(frame: Frame): void {
    this.emitted++;
    this.emit(frame);
  }
}

/** A stream that exists on the server and whose frames can be read. */
export interface StreamTarget {
  readonly streamId: string;
  /** `GET .../frames/stream` with the priority filter and any caller query parameters. */
  readonly url: URL;
  /** Frames the server generated, when it reported them. */
  readonly expectedFrames: number | undefined;
}

/** The stream the transport is currently driving. */
interface ActiveStream {
  readonly controller: AbortController;
  readonly reject: (error: PJSError) => void;
}

/**
 * HTTP transport implementation.
 *
 * Requires `globalThis.fetch` (browsers, Node 22+).
 */
export class HttpTransport extends Transport {
  private active?: ActiveStream;

  async connect(): Promise<ConnectResult> {
    try {
      const data = await this.requestJson(`${this.config.baseUrl}/pjs/sessions`, { client_info: CLIENT_INFO });
      const sessionId = field(data, 'session_id');
      if (typeof sessionId !== 'string') {
        throw new PJSError(PJSErrorType.ProtocolError, 'Session response has no session_id');
      }
      this.isConnected = true;
      return { sessionId };
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
    this.endActive(new PJSError(PJSErrorType.ConnectionError, 'Client disconnected'));
    this.isConnected = false;
  }

  /**
   * Streams `options.data` through the server. `endpoint` is ignored: the
   * routes are fixed. Resolves when the page is read; a later call rejects
   * this one with a `StreamError`.
   */
  async startStream(_endpoint: string, options: StreamOptions): Promise<void> {
    if (!this.isConnected) {
      throw new PJSError(PJSErrorType.ConnectionError, 'Transport not connected');
    }
    if (options.data === undefined) {
      throw new PJSError(
        PJSErrorType.ConfigurationError,
        'The HTTP transport requires StreamOptions.data (the document to stream)'
      );
    }

    this.endActive(new PJSError(PJSErrorType.StreamError, 'Stream superseded'));
    const controller = new AbortController();
    const ended = new Promise<never>((_, reject) => {
      this.active = { controller, reject };
    });

    try {
      await Promise.race([this.runStream(options.data, options, controller.signal), ended]);
    } catch (error) {
      if (controller.signal.aborted) {
        throw error;
      }
      const failure = error instanceof PJSError
        ? error
        : new PJSError(
          PJSErrorType.ConnectionError,
          'Failed to start HTTP stream',
          { sessionId: options.sessionId },
          error as Error
        );
      this.emitError(failure);
      throw failure;
    } finally {
      if (this.active?.controller === controller) {
        this.active = undefined;
      }
    }
  }

  async stopStream(): Promise<void> {
    this.endActive(new PJSError(PJSErrorType.StreamError, 'Stream stopped'));
  }

  /** Reads the frames of `target` and emits them through `ledger`; settles when the page is read. */
  protected async readFrames(
    target: StreamTarget,
    ledger: FrameLedger,
    options: StreamOptions,
    signal: AbortSignal
  ): Promise<void> {
    const response = await this.makeRequest(target.url.toString(), {
      method: 'GET',
      headers: {
        'Accept': 'application/x-ndjson',
        'Cache-Control': 'no-cache',
        ...this.config.headers,
        ...options.headers
      },
      signal
    });
    this.ensureOk(response);
    await this.processStreamingResponse(response, ledger);
  }

  /** Reports a malformed frame; `line` is the offending text. */
  protected emitParseError(message: string, line: string, cause: Error): void {
    this.emitError(new PJSError(PJSErrorType.ParseError, message, { line }, cause));
  }

  private async runStream(data: JsonValue, options: StreamOptions, signal: AbortSignal): Promise<void> {
    const target = await this.prepareStream(data, options, signal);
    await this.readFrames(target, new FrameLedger(target.streamId, (frame) => this.emitFrame(frame)), options, signal);
  }

  /** Creates and starts the stream, then asks the server to generate its frames. */
  private async prepareStream(data: JsonValue, options: StreamOptions, signal: AbortSignal): Promise<StreamTarget> {
    const headers = { ...this.config.headers, ...options.headers };
    const streams = `${this.config.baseUrl}/pjs/sessions/${encodeURIComponent(options.sessionId)}/streams`;

    const created = await this.requestJson(streams, { data }, headers, signal);
    const streamId = field(created, 'stream_id');
    if (typeof streamId !== 'string') {
      throw new PJSError(PJSErrorType.ProtocolError, 'Stream response has no stream_id');
    }

    const stream = `${streams}/${encodeURIComponent(streamId)}`;
    await this.requestJson(`${stream}/start`, undefined, headers, signal);
    const generated = await this.requestJson(
      `${stream}/generate-frames`,
      { priority_threshold: this.config.priorityThreshold, max_frames: MAX_FRAMES },
      headers,
      signal
    );

    const url = new URL(`${stream}/frames/stream`);
    url.searchParams.set('priority', String(this.config.priorityThreshold));
    Object.entries(options.queryParams ?? {}).forEach(([key, value]) => url.searchParams.set(key, value));
    const frameCount = field(generated, 'frame_count');
    return { streamId, url, expectedFrames: typeof frameCount === 'number' ? frameCount : undefined };
  }

  /** Rejects the active stream with `error` and aborts its requests; no-op without one. */
  private endActive(error: PJSError): void {
    const stream = this.active;
    this.active = undefined;
    if (stream) {
      stream.reject(error);
      stream.controller.abort();
    }
  }

  private async requestJson(
    url: string,
    body: unknown,
    headers: Record<string, string> = this.config.headers,
    signal?: AbortSignal
  ): Promise<unknown> {
    const timeout = AbortSignal.timeout(this.config.timeout);
    const response = await this.makeRequest(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...headers },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: signal ? AbortSignal.any([signal, timeout]) : timeout
    });
    this.ensureOk(response);
    return response.json() as Promise<unknown>;
  }

  private ensureOk(response: Response): void {
    if (!response.ok) {
      throw new PJSError(PJSErrorType.ConnectionError, `HTTP ${response.status}: ${response.statusText}`);
    }
  }

  private async makeRequest(url: string, options: RequestInit): Promise<Response> {
    if (this.config.debug) {
      console.log('[PJS HTTP] Making request:', { url, method: options.method });
    }

    return fetch(url, options);
  }

  private async processStreamingResponse(response: Response, ledger: FrameLedger): Promise<void> {
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
          if (line.trim() && this.handleLine(line, ledger, 'Failed to parse frame from HTTP response')) {
            await reader.cancel();
            return;
          }
        }
      }

      if (buffer.trim() && this.handleLine(buffer, ledger, 'Failed to parse final frame from HTTP response')) {
        await reader.cancel();
        return;
      }
      ledger.finish();

    } finally {
      reader.releaseLock();
    }
  }

  /** Pushes the line as a frame or reports a parse error; returns true when it was the shutdown marker. */
  private handleLine(line: string, ledger: FrameLedger, parseErrorMessage: string): boolean {
    let parsed: unknown;
    try {
      parsed = JSON.parse(line);
    } catch (error) {
      this.emitParseError(parseErrorMessage, line, error as Error);
      return false;
    }
    if (isShutdownMarker(parsed)) {
      this.emitShutdown();
      return true;
    }
    try {
      ledger.push(parsed);
    } catch (error) {
      this.emitParseError(parseErrorMessage, line, error as Error);
    }
    return false;
  }
}
