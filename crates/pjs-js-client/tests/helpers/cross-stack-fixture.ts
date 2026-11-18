/**
 * Starts and stops the `js_client_ws_fixture` example server for the
 * cross-stack suites. The binary is named by `PJS_WS_FIXTURE_BIN` and prints
 * `PJS_WS_FIXTURE_LISTENING <addr>` once it is bound; closing its stdin stops it.
 */

import { type ChildProcess, spawn } from 'node:child_process';

export const LISTENING_MARKER = 'PJS_WS_FIXTURE_LISTENING ';

export function startFixture(): Promise<{ child: ChildProcess; address: string }> {
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

export function stopFixture(child: ChildProcess): Promise<void> {
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
