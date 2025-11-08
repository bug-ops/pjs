/**
 * WASM Parser Integration
 *
 * Optional WebAssembly-accelerated JSON parsing using pjs-wasm.
 * Falls back to native JSON.parse() if WASM is not available.
 */

import { PJSError, PJSErrorType } from '../types/index.js';

export interface WasmParserOptions {
  debug?: boolean;
  preferNative?: boolean; // Force use of native JSON.parse
}

/**
 * WASM Parser instance
 *
 * Provides high-performance JSON parsing using Rust-based WASM module.
 */
export class WasmParser {
  private parser: any = null;
  private wasmAvailable = false;
  private options: Required<WasmParserOptions>;

  constructor(options: WasmParserOptions = {}) {
    this.options = {
      debug: false,
      preferNative: false,
      ...options
    };
  }

  /**
   * Initialize WASM module (async)
   *
   * @returns true if WASM initialized successfully, false if falling back to native
   */
  async initialize(): Promise<boolean> {
    if (this.options.preferNative) {
      if (this.options.debug) {
        console.log('[PJS WASM] Using native JSON.parse (preferNative=true)');
      }
      return false;
    }

    try {
      // Dynamic import - won't fail if pjs-wasm is not installed
      const wasmModule = await import('pjs-wasm');

      // Initialize WASM (calls wasm_bindgen start function)
      await wasmModule.default();

      // Create parser instance
      this.parser = new wasmModule.PjsParser();
      this.wasmAvailable = true;

      if (this.options.debug) {
        const version = wasmModule.version();
        console.log(`[PJS WASM] Initialized successfully (v${version})`);
      }

      return true;

    } catch (error) {
      if (this.options.debug) {
        console.warn('[PJS WASM] Not available, falling back to native JSON.parse:', error);
      }
      this.wasmAvailable = false;
      return false;
    }
  }

  /**
   * Parse JSON string
   *
   * @param jsonString - JSON string to parse
   * @returns Parsed JSON object
   * @throws PJSError if parsing fails
   */
  parse<T = any>(jsonString: string): T {
    try {
      if (this.wasmAvailable && this.parser) {
        // Use WASM parser
        return this.parser.parse(jsonString);
      } else {
        // Fallback to native JSON.parse
        return JSON.parse(jsonString);
      }
    } catch (error) {
      throw new PJSError(
        PJSErrorType.ParseError,
        'Failed to parse JSON',
        { inputLength: jsonString.length },
        error as Error
      );
    }
  }

  /**
   * Check if WASM is available and initialized
   */
  isWasmAvailable(): boolean {
    return this.wasmAvailable;
  }

  /**
   * Get parser implementation name
   */
  getImplementation(): 'wasm' | 'native' {
    return this.wasmAvailable ? 'wasm' : 'native';
  }

  /**
   * Dispose of WASM resources
   */
  dispose(): void {
    if (this.parser && typeof this.parser.free === 'function') {
      this.parser.free();
      this.parser = null;
    }
    this.wasmAvailable = false;
  }
}

/**
 * Create and initialize a WASM parser
 *
 * @param options - Parser options
 * @returns Initialized parser instance
 *
 * @example
 * ```typescript
 * const parser = await createWasmParser({ debug: true });
 * const data = parser.parse('{"name": "test"}');
 * console.log(`Using ${parser.getImplementation()} parser`);
 * ```
 */
export async function createWasmParser(
  options: WasmParserOptions = {}
): Promise<WasmParser> {
  const parser = new WasmParser(options);
  await parser.initialize();
  return parser;
}
