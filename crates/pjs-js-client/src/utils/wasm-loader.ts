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
 *
 * `pjs-wasm`'s `pkg/` output (built via `wasm-pack build --target web`) is
 * pure ES module syntax. A real `import('pjs-wasm')` works fine in plain
 * Node.js, but under this project's Jest setup, `import()` resolves
 * node_modules packages through Jest's own CommonJS-based module loader —
 * even though Jest's ambient `jest`/`describe`/`expect` globals (used
 * throughout this suite) already work fine without `--experimental-vm-
 * modules` — and that loader cannot parse the `export` keyword, throwing
 * `SyntaxError: Unexpected token 'export'`. Enabling `--experimental-vm-
 * modules` to get Jest to treat `pjs-wasm` as real ESM was tried and
 * rejected: it requires *every* test file to import `jest`/`describe`/etc.
 * from `@jest/globals` instead of relying on the ambient globals, which is
 * a suite-wide migration far outside this fix's scope. To get one loading
 * strategy that behaves identically under Jest and in real Node, Node.js
 * always loads the glue file's source directly and evaluates it as
 * CommonJS instead of using `import()`.
 */
function isNodeRuntime(): boolean {
  return typeof process !== 'undefined' && !!process.versions?.node;
}

// Cached across calls so WasmParser and WasmBackend initializing separately
// in the same process don't each read the binary or re-evaluate the module.
// Caches the in-flight *promise*, not the resolved value: caching only the
// resolved value lets two concurrent loadWasmModule() calls each `new
// Function` and initialize their own independent wasm instance, one of
// which becomes orphaned from whatever this module object gets cached to
// (and from anything that later patches properties on it, e.g. tests).
let cachedWasmBuffer: Buffer | null = null;
let cachedNodeModulePromise: Promise<typeof import('pjs-wasm')> | null = null;

/**
 * Rewrites wasm-bindgen's `--target web` output — `export class`/`export
 * function` declarations plus one trailing `export { a, b as c };` — into a
 * CommonJS module body, and evaluates it via the standard
 * `(exports, require, module, __filename, __dirname)` wrapper. Only the two
 * export forms wasm-bindgen actually emits are supported; anything else
 * throws rather than silently producing a broken module.
 */
function evaluateEsmGlueAsCjs(
  source: string,
  filename: string,
  dirname: string,
  esmUrl: string,
  req: NodeRequire
): Record<string, unknown> {
  const exportListMatch = source.match(/export\s*\{([^}]*)\}\s*;?\s*$/);
  if (!exportListMatch || exportListMatch.index === undefined) {
    throw new Error(
      `pjs-wasm's generated module has an unexpected shape: no trailing "export { ... }" statement found in ${filename}`
    );
  }

  const withoutExportList =
    source.slice(0, exportListMatch.index) + source.slice(exportListMatch.index + exportListMatch[0].length);

  const declExportPattern = /^export\s+(?:class|function|async\s+function|const|let)\s+([A-Za-z_$][\w$]*)/gm;
  const declExportedNames: string[] = [];
  for (const m of withoutExportList.matchAll(declExportPattern)) {
    declExportedNames.push(m[1]);
  }

  const cjsBody = withoutExportList
    .replace(/^export\s+(?=(?:class|function|async\s+function|const|let)\b)/gm, '')
    // Dead branch when init() is always called with an explicit buffer (as
    // this loader does), but still needs to be syntactically valid outside
    // of an ES module.
    .replace(/import\.meta\.url/g, JSON.stringify(esmUrl));

  const aggregateExportAssignments = exportListMatch[1]
    .split(',')
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => {
      const [local, alias] = entry.split(/\s+as\s+/).map((part) => part.trim());
      return `exports[${JSON.stringify(alias ?? local)}] = ${local};`;
    });

  const declExportAssignments = declExportedNames.map((name) => `exports[${JSON.stringify(name)}] = ${name};`);

  const exportAssignments = [...declExportAssignments, ...aggregateExportAssignments].join('\n');

  // jsdom (this project's Jest testEnvironment) doesn't expose TextEncoder
  // /TextDecoder as globals the way real Node.js and browsers do; the glue
  // uses both unconditionally at module-evaluation time. `'use strict'` is
  // needed because `new Function` bodies are sloppy-mode by default, while
  // the real ES module this replaces is always strict.
  const globalsPreamble = "'use strict';\nconst { TextEncoder, TextDecoder } = require('util');\n";

  const moduleFactory = new Function(
    'exports',
    'require',
    'module',
    '__filename',
    '__dirname',
    `${globalsPreamble}${cjsBody}\n${exportAssignments}`
  );

  const moduleShim = { exports: {} as Record<string, unknown> };
  moduleFactory(moduleShim.exports, req, moduleShim, filename, dirname);
  return moduleShim.exports;
}

async function loadWasmModuleForNode(): Promise<typeof import('pjs-wasm')> {
  // `require` is only available under CommonJS interop (Jest, CJS builds,
  // or ESM consumers with a `require` shim). A pure-ESM Node context
  // without one cannot resolve the on-disk package location this way.
  if (typeof require !== 'function') {
    throw new Error(
      'Cannot load pjs-wasm in this Node.js environment: no CommonJS `require` is ' +
      'available to resolve the installed package location. Loading pjs-wasm from a ' +
      'pure-ESM Node context without CJS interop is not yet supported.'
    );
  }

  if (!cachedNodeModulePromise) {
    cachedNodeModulePromise = (async () => {
      const { readFile } = await import('fs/promises');
      const { dirname, join } = await import('path');
      const { pathToFileURL } = await import('url');

      const pkgPath = require.resolve('pjs-wasm/package.json');
      const pkg = require(pkgPath) as { main?: unknown };
      if (typeof pkg.main !== 'string') {
        throw new Error(`pjs-wasm's package.json has no usable "main" field (got ${JSON.stringify(pkg.main)})`);
      }

      const pkgDir = dirname(pkgPath);
      const mainPath = join(pkgDir, pkg.main);
      const source = await readFile(mainPath, 'utf8');
      const moduleExports = evaluateEsmGlueAsCjs(source, mainPath, pkgDir, pathToFileURL(mainPath).href, require);

      const wasmPath = join(pkgDir, pkg.main.replace(/\.js$/, '_bg.wasm'));
      cachedWasmBuffer ??= await readFile(wasmPath);

      // Object form (not the deprecated positional `default(buffer)`) so the
      // glue's own destructuring path is used instead of its
      // deprecated-parameters warning branch.
      await (moduleExports.default as (init: { module_or_path: Buffer }) => Promise<unknown>)({
        module_or_path: cachedWasmBuffer
      });

      return moduleExports as typeof import('pjs-wasm');
    })().catch((error) => {
      // Don't cache a permanent failure — a later, non-concurrent call
      // should be able to retry.
      cachedNodeModulePromise = null;
      throw new Error(
        `Failed to load the pjs-wasm binary for Node.js: ${(error as Error).message}`,
        { cause: error }
      );
    });
  }

  return cachedNodeModulePromise;
}

/**
 * Import and initialize the pjs-wasm module, selecting the correct
 * initialization strategy for the current runtime.
 */
export async function loadWasmModule(): Promise<typeof import('pjs-wasm')> {
  if (isNodeRuntime()) {
    return loadWasmModuleForNode();
  }

  const wasmModule = await import('pjs-wasm');
  await wasmModule.default();
  return wasmModule;
}
