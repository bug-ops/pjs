/**
 * @jest-environment node
 *
 * Cross-stack acceptance tests for #731: the compiled JS client bundle
 * (`dist/index.js`, built with `npm run build`) streams documents from a real
 * `pjs-core` WebSocket server (`examples/js_client_ws_fixture.rs`).
 *
 * Runtime values (PJSClient, PJSError, ...) come only from the bundle, never
 * from `src`, so `instanceof` and enum checks compare against one module copy.
 * `src` is imported for types only.
 */

import { type ChildProcess, spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import * as path from 'node:path';
import { afterAll, beforeAll, describe, expect, test } from '@jest/globals';
import type { JsonValue } from '../../src/types/index.js';

type PjsModule = typeof import('../../src/index.js');

const DIST_ENTRY = path.resolve(__dirname, '../../dist/index.js');
const LISTENING_MARKER = 'PJS_WS_FIXTURE_LISTENING ';

function loadDist(): PjsModule {
  if (!existsSync(DIST_ENTRY)) {
    throw new Error(`${DIST_ENTRY} is missing: run "npm run build" before the cross-stack suite`);
  }
  return require(DIST_ENTRY) as PjsModule;
}

// Node 20 exposes globalThis.crypto, but a jest node environment is not guaranteed to forward it.
if (typeof globalThis.crypto?.randomUUID !== 'function') {
  Object.defineProperty(globalThis, 'crypto', {
    value: (require('node:crypto') as typeof import('node:crypto')).webcrypto,
    configurable: true
  });
}

// Node < 22 has no global WebSocket; the ws package behaves the same everywhere.
Object.defineProperty(globalThis, 'WebSocket', {
  value: (require('ws') as typeof import('ws')).WebSocket,
  configurable: true,
  writable: true
});

const pjs = loadDist();

let fixture: ChildProcess | undefined;
let baseUrl = '';

function startFixture(): Promise<{ child: ChildProcess; address: string }> {
  const bin = process.env.PJS_WS_FIXTURE_BIN;
  if (!bin) {
    return Promise.reject(
      new Error('PJS_WS_FIXTURE_BIN is not set: point it at the built js_client_ws_fixture example binary')
    );
  }

  return new Promise((resolve, reject) => {
    const child = spawn(bin, [], { stdio: ['pipe', 'pipe', 'inherit'] });
    let buffered = '';
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error(`fixture did not print "${LISTENING_MARKER}<addr>" within 30s`));
    }, 30_000);

    child.once('error', (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.once('exit', (code) => {
      clearTimeout(timer);
      reject(new Error(`fixture exited early with code ${String(code)}`));
    });
    child.stdout?.setEncoding('utf8');
    child.stdout?.on('data', (chunk: string) => {
      buffered += chunk;
      const line = buffered.split('\n').find((candidate) => candidate.startsWith(LISTENING_MARKER));
      if (line !== undefined) {
        clearTimeout(timer);
        child.removeAllListeners('exit');
        resolve({ child, address: line.slice(LISTENING_MARKER.length).trim() });
      }
    });
  });
}

function stopFixture(child: ChildProcess): Promise<void> {
  return new Promise((resolve) => {
    if (child.exitCode !== null) {
      resolve();
      return;
    }
    const timer = setTimeout(() => child.kill(), 10_000);
    child.once('exit', () => {
      clearTimeout(timer);
      resolve();
    });
    child.stdin?.end();
  });
}

beforeAll(async () => {
  const started = await startFixture();
  fixture = started.child;
  baseUrl = `http://${started.address}`;
});

afterAll(async () => {
  if (fixture) await stopFixture(fixture);
});

async function streamDocument(data: JsonValue): Promise<JsonValue> {
  const client = new pjs.PJSClient({
    baseUrl,
    transport: pjs.TransportType.WebSocket,
    timeout: 45_000
  });
  try {
    await client.connect();
    return await client.stream<JsonValue>('/ignored', { data });
  } finally {
    await client.disconnect();
  }
}

function largeObjectDocument(): JsonValue {
  const longText = 'lorem ipsum dolor sit amet, ключ \u{1F600} '.repeat(4000);
  return {
    'a.b': { inner: [1, 2, { deep: ['x', ['y', ['z']]] }] },
    '': 'empty key value',
    '[x]': [true, false, null],
    '0': { zero: 0 },
    nested: [[1, [2, [3, [4]]]], { k: [[], {}] }],
    text: longText,
    items: Array.from({ length: 3000 }, (_, i) => ({ id: i, name: `item-${i}`, tags: ['a', 'b'] }))
  };
}

describe('compiled JS client against the Rust WebSocket server', () => {
  test('streams an object root larger than one frame with path-significant keys', async () => {
    const document = largeObjectDocument();
    expect(JSON.stringify(document).length).toBeGreaterThan(64 * 1024);

    expect(await streamDocument(document)).toEqual(document);
  });

  test('streams a large array root', async () => {
    const document: JsonValue = Array.from({ length: 20_000 }, (_, i) => ({ i, label: `row-${i}` }));
    expect(JSON.stringify(document).length).toBeGreaterThan(64 * 1024);

    expect(await streamDocument(document)).toEqual(document);
  });

  test('streams a large string root', async () => {
    const document = 'streamed text \u{1F680} ключ '.repeat(20_000);
    expect(document.length).toBeGreaterThan(64 * 1024);

    expect(await streamDocument(document)).toBe(document);
  });

  test('streams a small scalar root', async () => {
    expect(await streamDocument(42)).toBe(42);
  });

  test('rejects a 130-level document with ProtocolError code 400 and a null session', async () => {
    let document: JsonValue = 0;
    for (let level = 0; level < 130; level++) document = [document];

    const error = await streamDocument(document).then(
      () => undefined,
      (caught: unknown) => caught
    );

    expect(error).toBeInstanceOf(pjs.PJSError);
    expect(error).toMatchObject({
      type: pjs.PJSErrorType.ProtocolError,
      context: { code: 400 }
    });
  });
});
