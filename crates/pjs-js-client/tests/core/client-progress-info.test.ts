/**
 * @jest-environment node
 *
 * Acceptance tests for #766 (track H2): `ProgressInfo` no longer carries the
 * always-zero `bytesReceived`, and `framesReceived` and `totalFrames` count the
 * same frames (skeleton + patches + complete) on every backend that reports a
 * total, so a finished stream compares equal.
 */

import { afterEach, beforeEach, describe, expect, jest, test } from '@jest/globals';
import { PJSClient } from '../../src/core/client.js';
import { WasmBackend } from '../../src/transport/wasm-backend.js';
import * as wasmLoader from '../../src/utils/wasm-loader.js';
import {
  type Frame,
  PJSEvent,
  Priority,
  type PJSClientConfig,
  type ProgressInfo,
  TransportType
} from '../../src/types/index.js';
import {
  FakeSocket,
  UUID_1,
  completeMessage,
  flush,
  installFakes,
  patchMessage,
  restoreFakes
} from '../helpers/fake-websocket.js';

beforeEach(() => installFakes(UUID_1));
afterEach(() => {
  restoreFakes();
  jest.restoreAllMocks();
});

async function completedProgress(): Promise<ProgressInfo> {
  const client = new PJSClient({
    baseUrl: 'http://localhost:3000',
    transport: TransportType.WebSocket,
    timeout: 2000
  });
  const connecting = client.connect();
  const socket = FakeSocket.latest();
  socket.open();
  await connecting;
  let completed: ProgressInfo | undefined;
  client.on(PJSEvent.StreamComplete, ({ stats }: { stats: ProgressInfo }) => {
    completed = { ...stats };
  });

  const pending = client.stream('/x', { data: { a: 1, b: 2 } });
  await flush();
  socket.receive(patchMessage(0, [['set', ['a'], 1]], UUID_1));
  socket.receive(patchMessage(1, [['set', ['b'], 2]], UUID_1));
  socket.receive(completeMessage(UUID_1));
  await pending;
  await client.disconnect();
  if (completed === undefined) throw new Error('no stream_complete event');
  return completed;
}

describe('ProgressInfo over the WebSocket transport', () => {
  test('framesReceived equals totalFrames when the stream completes', async () => {
    const progress = await completedProgress();

    // skeleton + two patch frames + complete
    expect(progress.framesReceived).toBe(4);
    expect(progress.totalFrames).toBe(4);
    expect(progress.completionPercentage).toBe(100);
  });

  test('bytesReceived is gone from the type and from the object', async () => {
    const progress = await completedProgress();

    // @ts-expect-error -- bytesReceived was removed from ProgressInfo
    const removed: unknown = progress.bytesReceived;

    expect(removed).toBeUndefined();
    expect(Object.keys(progress)).not.toContain('bytesReceived');
  });
});

describe('Complete frame total_frames of the WASM backend', () => {
  function frameData(type: string, sequence: number, payload: unknown) {
    return { type, priority: Priority.High, sequence, payload: JSON.stringify(payload) };
  }

  test('counts the frames it emitted, not the figure the module reports', async () => {
    let onFrame: (frame: unknown) => void = () => {};
    let onComplete: (stats: unknown) => void = () => {};
    class FakePriorityStream {
      setMinPriority(): void {}
      onFrame(callback: (frame: unknown) => void): void {
        onFrame = callback;
      }
      onComplete(callback: (stats: unknown) => void): void {
        onComplete = callback;
      }
      onError(): void {}
      free(): void {}
      start(): void {
        onFrame(frameData('skeleton', 0, { a: null }));
        onFrame(frameData('patch', 1, { patches: [{ operation: 'set', path: '$.a', value: 1 }] }));
        onFrame(frameData('patch', 2, { patches: [{ operation: 'set', path: '$.b', value: 2 }] }));
        onComplete({ totalFrames: 999, patchFrames: 2, bytesProcessed: 10, durationMs: 1 });
      }
    }
    jest.spyOn(wasmLoader, 'loadWasmModule').mockResolvedValue({
      PriorityStream: FakePriorityStream,
      version: () => 'test'
    } as never);
    const config: Required<PJSClientConfig> = {
      baseUrl: 'wasm://local',
      transport: TransportType.WASM,
      sessionId: '',
      headers: {},
      timeout: 1000,
      bufferSize: 1024,
      priorityThreshold: Priority.Background,
      maxConcurrentStreams: 1,
      debug: false
    } as Required<PJSClientConfig>;
    const backend = new WasmBackend(config);
    const frames: Frame[] = [];
    backend.on('frame', (frame: Frame) => frames.push(frame));
    await backend.connect();

    await backend.startStream('local', { sessionId: 'wasm-local', streamId: 's', jsonData: '{"a":1,"b":2}' });

    const complete = frames[frames.length - 1];
    expect(complete.frame_type).toBe('Complete');
    expect(frames).toHaveLength(4);
    expect((complete as { total_frames?: number }).total_frames).toBe(4);
  });
});
