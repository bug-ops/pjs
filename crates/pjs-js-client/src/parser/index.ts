/**
 * Parser Module
 *
 * Provides JSON parsing capabilities with optional WASM acceleration.
 */

export { WasmParser, createWasmParser } from './wasm-parser.js';
export type { WasmParserOptions, StreamingCallbacks } from './wasm-parser.js';
