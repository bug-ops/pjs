/**
 * WASM Module Loader
 *
 * Loads and initializes the pjs-wasm module for the current JavaScript runtime.
 *
 * The generated `pjs-wasm` init function defaults to fetching the `.wasm`
 * binary via `fetch(new URL(...))` when called with no argument. Node's
 * built-in `fetch` cannot load `file:` URLs, so in Node.js the binary is
 * read from disk and passed directly to skip the fetch path. In browsers,
 * the default fetch-based loading path is used unchanged.
 */
function isNodeRuntime(): boolean {
  return typeof process !== 'undefined' && !!process.versions?.node;
}

// Cached across calls so WasmParser and WasmBackend initializing separately
// in the same process don't each read the binary from disk.
let cachedWasmBuffer: Buffer | null = null;

/**
 * Import and initialize the pjs-wasm module, selecting the correct
 * initialization strategy for the current runtime.
 */
export async function loadWasmModule(): Promise<typeof import('pjs-wasm')> {
  const wasmModule = await import('pjs-wasm');

  if (isNodeRuntime()) {
    // `require` is only available under CommonJS interop (Jest, CJS builds,
    // or ESM consumers with a `require` shim). A pure-ESM Node context
    // without one cannot resolve the on-disk binary path this way.
    if (typeof require !== 'function') {
      throw new Error(
        'Cannot load pjs-wasm in this Node.js environment: no CommonJS `require` is ' +
        'available to resolve the installed package location. Loading pjs-wasm from a ' +
        'pure-ESM Node context without CJS interop is not yet supported.'
      );
    }

    try {
      const { readFile } = await import('fs/promises');
      const { dirname, join } = await import('path');

      const pkgPath = require.resolve('pjs-wasm/package.json');
      const pkg = require(pkgPath) as { main?: unknown };
      if (typeof pkg.main !== 'string') {
        throw new Error(`pjs-wasm's package.json has no usable "main" field (got ${JSON.stringify(pkg.main)})`);
      }

      const wasmPath = join(dirname(pkgPath), pkg.main.replace(/\.js$/, '_bg.wasm'));
      cachedWasmBuffer ??= await readFile(wasmPath);

      await wasmModule.default(cachedWasmBuffer);
    } catch (error) {
      throw new Error(
        `Failed to load the pjs-wasm binary for Node.js: ${(error as Error).message}`,
        { cause: error }
      );
    }
  } else {
    await wasmModule.default();
  }

  return wasmModule;
}
