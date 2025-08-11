/**
 * PJS JavaScript/TypeScript Client Library
 * 
 * Priority JSON Streaming Protocol (PJS) client for JavaScript and TypeScript applications.
 * Provides progressive JSON loading with priority-based rendering for optimal user experience.
 * 
 * @example
 * ```typescript
 * import { PJSClient, Priority } from '@pjs/client';
 * 
 * const client = new PJSClient({
 *   baseUrl: 'http://localhost:3000',
 *   transport: 'http'
 * });
 * 
 * // Stream JSON with progress callbacks
 * const data = await client.stream('/api/data', {
 *   onRender: (partialData, metadata) => {
 *     if (metadata.priority >= Priority.High) {
 *       updateUI(partialData);
 *     }
 *   },
 *   onProgress: (progress) => {
 *     console.log(`${progress.completionPercentage}% complete`);
 *   }
 * });
 * 
 * console.log('Complete data:', data);
 * ```
 * 
 * @version 0.3.0-alpha.1
 * @license MIT OR Apache-2.0
 */

// Main client class
export { PJSClient } from './core/client.js';

// Core processing classes
export { FrameProcessor } from './core/frame-processor.js';
export { JsonReconstructor } from './core/json-reconstructor.js';

// Transport implementations
export { Transport } from './transport/base.js';
export { HttpTransport } from './transport/http.js';
export { WebSocketTransport } from './transport/websocket.js';
export { SSETransport } from './transport/sse.js';

// Type definitions
export * from './types/index.js';

// Utility functions
export { createPJSClient, validateFrame, parseJsonPath } from './utils/index.js';

/**
 * Library version
 */
export const VERSION = '0.3.0-alpha.1';

/**
 * Default client configuration
 */
export const DEFAULT_CONFIG = {
  timeout: 30000,
  bufferSize: 1024 * 1024, // 1MB
  debug: false,
  priorityThreshold: 10, // Priority.Background
  maxConcurrentStreams: 10
} as const;

/**
 * Quick start function to create a PJS client with minimal configuration
 * 
 * @param baseUrl - PJS server base URL
 * @param options - Additional client options
 * @returns Configured PJS client instance
 * 
 * @example
 * ```typescript
 * import { quickStart } from '@pjs/client';
 * 
 * const client = quickStart('http://localhost:3000');
 * const data = await client.stream('/api/users');
 * ```
 */
export function quickStart(
  baseUrl: string, 
  options: Partial<import('./types/index.js').PJSClientConfig> = {}
): import('./core/client.js').PJSClient {
  const { PJSClient } = require('./core/client.js');
  
  return new PJSClient({
    baseUrl,
    ...DEFAULT_CONFIG,
    ...options
  });
}