/**
 * Server-Sent Events Transport - the HTTP transport with an `EventSource`
 * reading `GET .../frames/stream`.
 *
 * Session and stream setup is the HTTP transport's. The server answers an
 * `EventSource` (default `Accept`) with `text/event-stream`, one `data:` frame
 * per event, and closes the body after the last frame without a final event.
 * `EventSource` would redial, so the transport treats the first `error` after
 * a clean open as the end of the page and closes the source itself.
 */

import { FrameLedger, HttpTransport, StreamTarget } from './http.js';
import { field } from './json-record.js';
import { SHUTDOWN_EVENT, StreamOptions } from './base.js';
import { PJSError, PJSErrorType } from '../types/index.js';

/**
 * Server-Sent Events transport implementation.
 *
 * Requires `globalThis.EventSource` (browsers or a polyfill) and `fetch`.
 * `EventSource` cannot set request headers, so `headers` apply to the REST
 * calls only.
 */
export class SSETransport extends HttpTransport {
  protected override readFrames(
    target: StreamTarget,
    ledger: FrameLedger,
    _options: StreamOptions,
    signal: AbortSignal
  ): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      const source = new EventSource(target.url.toString());
      let opened = false;
      let ended = false;

      const end = (error?: PJSError): void => {
        if (ended) return;
        ended = true;
        source.close();
        signal.removeEventListener('abort', onAbort);
        if (error) {
          reject(error);
        } else {
          resolve();
        }
      };
      // Closing the source prevents EventSource from redialing; the caller already rejected the stream.
      const onAbort = (): void => end(new PJSError(PJSErrorType.StreamError, 'Stream ended'));
      signal.addEventListener('abort', onAbort, { once: true });

      const handleMessage = (data: string): void => {
        if (ended) return;
        try {
          ledger.push(JSON.parse(data));
        } catch (error) {
          this.emitParseError('Failed to parse SSE frame', data, error as Error);
          return;
        }
        if (ledger.completed) end();
      };

      source.onopen = () => {
        opened = true;
        if (this.config.debug) {
          console.log('[PJS SSE] Stream opened:', target.url.toString());
        }
      };
      source.onmessage = (event) => handleMessage(event.data as string);
      source.addEventListener('frame', (event) => handleMessage((event as MessageEvent<string>).data));
      source.onerror = () => {
        if (ended) return;
        const complete = opened && (target.expectedFrames === undefined || ledger.received >= target.expectedFrames);
        if (complete) {
          ledger.finish();
          end();
        } else {
          end(new PJSError(PJSErrorType.ConnectionError, 'SSE connection error', { url: target.url.toString() }));
        }
      };
      // A server-sent `event: error` carries data; connection errors (no data) are handled by onerror.
      source.addEventListener('error', (event) => {
        const data = (event as MessageEvent<unknown>).data;
        if (typeof data !== 'string') return;
        let details: unknown;
        try {
          details = JSON.parse(data);
        } catch {
          details = { data };
        }
        const message = field(details, 'message');
        this.emitError(new PJSError(
          PJSErrorType.ProtocolError,
          typeof message === 'string' ? message : 'Server error',
          details
        ));
      });
      // Closing the source prevents EventSource from auto-reconnecting to a stopping server.
      source.addEventListener(SHUTDOWN_EVENT, () => {
        if (ended) return;
        end();
        this.emitShutdown();
      });
    });
  }
}
