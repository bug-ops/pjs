/**
 * PatchApplied event carries per-frame appliedPatches / skippedPatches (#712).
 */

import { describe, test, expect, jest } from '@jest/globals';
import { PJSClient } from '../../src/core/client.js';
import type { Transport } from '../../src/transport/base.js';
import { FrameType, Priority, PJSEvent, type Frame, type PatchOperation } from '../../src/types/index.js';

type Counts = { appliedPatches: number; skippedPatches: number };

function patchFrame(patches: PatchOperation[]): Frame {
  return {
    frame_type: FrameType.Patch as FrameType.Patch,
    priority: Priority.High,
    payload: { patches },
    timestamp: 0
  };
}

async function streamAndCollect(skeleton: unknown, patchFrames: Frame[]): Promise<Counts[]> {
  const client = new PJSClient({ baseUrl: 'http://localhost:3000', debug: false });
  const counts: Counts[] = [];
  client.on(PJSEvent.PatchApplied, (event) => {
    counts.push({ appliedPatches: event.appliedPatches, skippedPatches: event.skippedPatches });
  });

  const frames: Frame[] = [
    {
      frame_type: FrameType.Skeleton,
      priority: Priority.Critical,
      payload: skeleton,
      complete: false,
      timestamp: 0
    } as Frame,
    ...patchFrames,
    { frame_type: FrameType.Complete as FrameType.Complete, priority: Priority.Background, timestamp: 0, payload: {} } as Frame
  ];

  jest.spyOn(client as unknown as { transport: Transport }, 'transport', 'get').mockReturnValue({
    connect: () => Promise.resolve({ sessionId: 'counts-session' }),
    startStream: () => {
      setTimeout(() => {
        for (const frame of frames) client.emit(PJSEvent.FrameReceived, { frame });
      }, 5);
      return Promise.resolve();
    },
    on: jest.fn(),
    removeListener: jest.fn(),
    disconnect: () => Promise.resolve()
  } as unknown as Transport);

  await client.stream('/api/counts');
  return counts;
}

describe('PatchApplied counts', () => {
  test('all patches applied', async () => {
    const counts = await streamAndCollect({ a: 0, b: 0 }, [
      patchFrame([
        { path: '$.a', value: 1, operation: 'set' },
        { path: '$.b', value: 2, operation: 'set' }
      ])
    ]);

    expect(counts).toEqual([{ appliedPatches: 2, skippedPatches: 0 }]);
  });

  test('skips missing and out-of-range index paths per frame', async () => {
    const counts = await streamAndCollect({ a: 0, items: [1] }, [
      patchFrame([
        { path: '$.a', value: 1, operation: 'set' },
        { path: '$.missing[0]', value: 1, operation: 'set' },
        { path: '$.items[5]', value: 1, operation: 'set' }
      ]),
      patchFrame([{ path: '$.a', value: 2, operation: 'set' }])
    ]);

    expect(counts).toEqual([
      { appliedPatches: 1, skippedPatches: 2 },
      { appliedPatches: 1, skippedPatches: 0 }
    ]);
  });

  test('prototype-named keys are applied, not skipped', async () => {
    const counts = await streamAndCollect({ id: 0 }, [
      patchFrame([
        { path: '$.constructor', value: 'Acme', operation: 'set' },
        { path: '$.__proto__.x', value: 1, operation: 'set' }
      ])
    ]);

    expect(counts).toEqual([{ appliedPatches: 2, skippedPatches: 0 }]);
    expect(({} as Record<string, unknown>).x).toBeUndefined();
  });

  test('root append counts as skipped', async () => {
    const counts = await streamAndCollect({ a: 0 }, [
      patchFrame([
        { path: '$', value: 1, operation: 'append' },
        { path: '$.a', value: 1, operation: 'set' }
      ])
    ]);

    expect(counts).toEqual([{ appliedPatches: 1, skippedPatches: 1 }]);
  });
});
