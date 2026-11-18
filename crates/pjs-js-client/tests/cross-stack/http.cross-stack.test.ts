/**
 * @jest-environment node
 *
 * Cross-stack acceptance tests for #735 (track H1): the compiled JS client
 * bundle (`dist/index.js`, built with `npm run build`) streams documents over
 * its HTTP transport from the stock `pjs-core` REST routes served by
 * `examples/js_client_ws_fixture.rs` (same binary and env var as the
 * WebSocket suite).
 *
 * Runtime values come only from the bundle; `src` is imported for types only.
 */

import { type ChildProcess } from 'node:child_process';
import { existsSync } from 'node:fs';
import * as path from 'node:path';
import { afterAll, beforeAll, describe, expect, test } from '@jest/globals';
import type { JsonValue, ProgressInfo } from '../../src/types/index.js';
import { startFixture, stopFixture } from '../helpers/cross-stack-fixture.js';

type PjsModule = typeof import('../../src/index.js');

const DIST_ENTRY = path.resolve(__dirname, '../../dist/index.js');

function loadDist(): PjsModule {
  if (!existsSync(DIST_ENTRY)) {
    throw new Error(`${DIST_ENTRY} is missing: run "npm run build" before the cross-stack suite`);
  }
  return require(DIST_ENTRY) as PjsModule;
}

const pjs = loadDist();

let fixture: ChildProcess | undefined;
let baseUrl = '';

beforeAll(async () => {
  const started = await startFixture();
  fixture = started.child;
  baseUrl = `http://${started.address}`;
});

afterAll(async () => {
  if (fixture) await stopFixture(fixture);
});

function newClient(): InstanceType<PjsModule['PJSClient']> {
  return new pjs.PJSClient({ baseUrl, transport: pjs.TransportType.HTTP, timeout: 45_000 });
}

async function streamDocument(data: JsonValue): Promise<JsonValue> {
  const client = newClient();
  try {
    await client.connect();
    return await client.stream<JsonValue>('/ignored', { data });
  } finally {
    await client.disconnect();
  }
}

describe('compiled JS client over HTTP against the Rust REST routes', () => {
  test('streams an object root with path-significant keys', async () => {
    const document: JsonValue = {
      'a.b': { inner: [1, 2, { deep: ['x', ['y']] }] },
      '': 'empty key value',
      '[x]': [true, false, null],
      id: 7,
      name: 'ada',
      tags: ['a', 'b', 'c']
    };

    expect(await streamDocument(document)).toEqual(document);
  });

  test('streams an array root', async () => {
    const document: JsonValue = Array.from({ length: 200 }, (_, i) => ({ i, label: `row-${i}` }));

    expect(await streamDocument(document)).toEqual(document);
  });

  test('streams a scalar root', async () => {
    expect(await streamDocument(42)).toBe(42);
  });

  test('reports the same frame count as received and total when the stream completes', async () => {
    const client = newClient();
    const completed: ProgressInfo[] = [];
    client.on(pjs.PJSEvent.StreamComplete, ({ stats }: { stats: ProgressInfo }) => {
      completed.push({ ...stats });
    });
    try {
      await client.connect();
      await client.stream('/ignored', { data: { id: 1, name: 'ada', tags: ['a', 'b'] } });
    } finally {
      await client.disconnect();
    }

    expect(completed).toHaveLength(1);
    expect(completed[0].framesReceived).toBeGreaterThanOrEqual(3);
    expect(completed[0].totalFrames).toBe(completed[0].framesReceived);
  });
});
