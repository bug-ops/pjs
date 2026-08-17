/**
 * Shared guard for the two `pjs-wasm`-backed integration suites
 * (`wasm-backend.test.ts`, `wasm-parser.test.ts`): whether `crates/pjs-wasm/pkg`
 * has been built, its version (for assertions that shouldn't hardcode it),
 * and a `describe` wrapper that reacts to a missing package appropriately
 * for the environment it runs in.
 */
import { describe, test } from '@jest/globals';
import { existsSync, readFileSync } from 'fs';
import { resolve } from 'path';

const wasmPkgPath = [
  resolve(process.cwd(), 'crates/pjs-wasm/pkg/package.json'),
  resolve(process.cwd(), '../pjs-wasm/pkg/package.json')
].find(existsSync);

export const wasmPkgAvailable = !!wasmPkgPath;

export const wasmPkgVersion: string | undefined = wasmPkgPath
  ? JSON.parse(readFileSync(wasmPkgPath, 'utf8')).version
  : undefined;

/**
 * `describe.skip`s the suite when `crates/pjs-wasm/pkg` isn't built, except
 * under CI (`process.env.CI`), where a missing artifact fails loudly instead
 * — a silent `describe.skip` in CI is exactly the failure mode that let
 * #383 survive two earlier fix attempts uncaught.
 */
export const describeWasmPkg: (name: string, fn: () => void) => void = wasmPkgAvailable
  ? describe
  : process.env.CI
    ? (name, _fn) =>
        describe(name, () => {
          test('crates/pjs-wasm/pkg must be built in CI', () => {
            throw new Error(
              'crates/pjs-wasm/pkg is missing. Run `wasm-pack build crates/pjs-wasm --target web` ' +
              '(or download the wasm-build-web CI artifact) before running these tests in CI.'
            );
          });
        })
    : describe.skip;
