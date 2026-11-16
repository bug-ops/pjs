/**
 * PJS Client Utilities
 * 
 * Helper functions for working with PJS protocol and client library.
 */

import {
  Frame,
  FrameType,
  JsonPath,
  PJSClientConfig,
  Priority,
  TransportType
} from '../types/index.js';
import type { PJSClient } from '../core/client.js';
import { tokenizeJsonPath } from '../core/json-path.js';

/**
 * Create a PJS client with smart defaults based on environment
 */
export function createPJSClient(config: PJSClientConfig): PJSClient {
  // eslint-disable-next-line @typescript-eslint/no-require-imports -- lazy load avoids a core/client import cycle
  const { PJSClient: Client } = require('../core/client.js');
  
  // Auto-detect best transport based on environment and URL
  if (!config.transport) {
    if (typeof WebSocket !== 'undefined' && config.baseUrl.startsWith('ws')) {
      config.transport = TransportType.WebSocket;
    } else if (typeof EventSource !== 'undefined') {
      config.transport = TransportType.ServerSentEvents;
    } else {
      config.transport = TransportType.HTTP;
    }
  }
  
  return new Client(config);
}

/**
 * Validate a frame according to PJS protocol specification
 */
export function validateFrame(frame: any): frame is Frame {
  try {
    if (!frame || typeof frame !== 'object') {
      return false;
    }

    // Check required fields
    if (!frame.frame_type || !Object.values(FrameType).includes(frame.frame_type)) {
      return false;
    }

    if (typeof frame.priority !== 'number') {
      return false;
    }

    // Type-specific validation
    switch (frame.frame_type) {
      case FrameType.Skeleton:
        return frame.payload !== undefined;

      case FrameType.Patch:
        return Array.isArray(frame.payload?.patches) && frame.payload.patches.length > 0;

      case FrameType.Complete:
        return true; // Complete frames are minimal
        
      default:
        return false;
    }
  } catch {
    return false;
  }
}

/**
 * Parse and validate a JSON path.
 *
 * `segments` are the dot-separated pieces after `$`, with indices kept on their piece
 * (`$.users[0].name` gives `['users[0]', 'name']`).
 */
export function parseJsonPath(path: JsonPath): {
  segments: string[];
  isValid: boolean;
  errors: string[];
} {
  const parsed = tokenizeJsonPath(path);
  if (!parsed.ok) {
    return { segments: [], isValid: false, errors: [parsed.error] };
  }

  const segments: string[] = [];
  let lastIsPiece = false;
  for (const segment of parsed.segments) {
    if (segment.kind === 'key') {
      segments.push(segment.key);
      lastIsPiece = true;
    } else if (lastIsPiece) {
      segments[segments.length - 1] += `[${segment.index}]`;
    } else {
      segments.push(`[${segment.index}]`);
      lastIsPiece = true;
    }
  }
  return { segments, isValid: true, errors: [] };
}

/**
 * Calculate priority based on JSON path and value heuristics
 */
export function calculateHeuristicPriority(path: JsonPath, value: any): Priority {
  const pathLower = path.toLowerCase();
  
  // Critical fields (user identity, status)
  if (pathLower.includes('id') && pathLower.includes('user')) return Priority.Critical;
  if (pathLower.includes('status') || pathLower.includes('state')) return Priority.Critical;
  if (pathLower.includes('error') || pathLower.includes('exception')) return Priority.Critical;
  
  // High priority fields (names, titles, key info)
  if (pathLower.includes('name') || pathLower.includes('title')) return Priority.High;
  if (pathLower.includes('email') || pathLower.includes('phone')) return Priority.High;
  if (pathLower.includes('price') || pathLower.includes('cost')) return Priority.High;
  
  // Default based on data type and size (checked before path keywords)
  if (typeof value === 'string' && value.length > 1000) return Priority.Low;

  // Medium priority (descriptive content)
  if (pathLower.includes('description') || pathLower.includes('summary')) return Priority.Medium;
  if (pathLower.includes('content') || pathLower.includes('body')) return Priority.Medium;

  // Low priority (metadata, timestamps)
  if (pathLower.includes('created') || pathLower.includes('updated')) return Priority.Low;
  if (pathLower.includes('metadata') || pathLower.includes('meta')) return Priority.Low;

  // Background priority (large arrays, detailed info)
  if (Array.isArray(value) && value.length > 10) return Priority.Background;
  if (pathLower.includes('analytics') || pathLower.includes('stats')) return Priority.Background;
  if (typeof value === 'object' && value !== null) {
    const keys = Object.keys(value);
    if (keys.length > 20) return Priority.Low;
  }
  
  return Priority.Medium; // Default priority
}

/**
 * Estimate the memory usage of a JavaScript object
 */
export function estimateObjectSize(obj: any): number {
  const seen = new WeakSet();
  
  function sizeOf(obj: any): number {
    if (obj === null || typeof obj !== 'object') {
      if (typeof obj === 'string') return obj.length * 2; // UTF-16
      if (typeof obj === 'number') return 8;
      if (typeof obj === 'boolean') return 4;
      return 0;
    }
    
    if (seen.has(obj)) return 0; // Circular reference
    seen.add(obj);
    
    let size = 0;
    
    if (Array.isArray(obj)) {
      size += obj.length * 8; // Array overhead
      for (const item of obj) {
        size += sizeOf(item);
      }
    } else {
      const keys = Object.keys(obj);
      size += keys.length * 8; // Object overhead
      
      for (const key of keys) {
        size += key.length * 2; // Key string
        size += sizeOf(obj[key]); // Value
      }
    }
    
    return size;
  }
  
  try {
    return sizeOf(obj);
  } catch {
    return 0;
  }
}

/**
 * Create a debounced function for UI updates
 */
export function debounce<T extends (...args: any[]) => void>(
  func: T,
  wait: number
): T & { cancel(): void } {
  let timeout: ReturnType<typeof setTimeout> | null = null;
  
  const debounced = ((...args: Parameters<T>) => {
    if (timeout) clearTimeout(timeout);
    timeout = setTimeout(() => func(...args), wait);
  }) as T & { cancel(): void };
  
  debounced.cancel = () => {
    if (timeout) {
      clearTimeout(timeout);
      timeout = null;
    }
  };
  
  return debounced;
}

/**
 * Create a throttled function for high-frequency events
 */
export function throttle<T extends (...args: any[]) => void>(
  func: T,
  wait: number
): T & { cancel(): void } {
  let timeout: ReturnType<typeof setTimeout> | null = null;
  let lastArgs: Parameters<T> | null = null;
  
  const throttled = ((...args: Parameters<T>) => {
    lastArgs = args;
    
    if (timeout) return;
    
    timeout = setTimeout(() => {
      if (lastArgs) func(...lastArgs);
      timeout = null;
      lastArgs = null;
    }, wait);
  }) as T & { cancel(): void };
  
  throttled.cancel = () => {
    if (timeout) {
      clearTimeout(timeout);
      timeout = null;
      lastArgs = null;
    }
  };
  
  return throttled;
}

/**
 * Format bytes into human-readable string
 */
export function formatBytes(bytes: number): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let size = bytes;
  let unitIndex = 0;
  
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex++;
  }
  
  return `${size.toFixed(1)} ${units[unitIndex]}`;
}

/**
 * Format duration into human-readable string
 */
export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  if (ms < 3600000) return `${Math.floor(ms / 60000)}m ${Math.floor((ms % 60000) / 1000)}s`;
  
  const hours = Math.floor(ms / 3600000);
  const minutes = Math.floor((ms % 3600000) / 60000);
  return `${hours}h ${minutes}m`;
}
