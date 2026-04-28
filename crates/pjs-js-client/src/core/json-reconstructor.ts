/**
 * JSON Reconstructor - Progressive JSON reconstruction from PJS frames
 *
 * Reconstructs complete JSON objects from skeleton and patch frames,
 * applying operations in priority order.
 */

import {
  JsonPath,
  SkeletonFrame,
  PatchFrame
} from '../types/index.js';

import { estimateObjectSize } from '../utils/index.js';

/**
 * Reconstructs JSON objects from PJS frames using efficient patching
 */
export class JsonReconstructor {
  private state: any = null;
  private initialized = false;
  private complete = false;
  private patchesApplied = 0;

  constructor() {}

  /**
   * Process a skeleton frame to initialize the reconstructor state.
   */
  processSkeleton(frame: SkeletonFrame): { success: boolean; data?: any; error?: string } {
    if (this.initialized) {
      return { success: false, error: 'Reconstructor already initialized' };
    }

    if ((frame as any).data === undefined) {
      return { success: false, error: 'Skeleton frame missing data field' };
    }

    this.state = this.deepClone(frame.data);
    this.initialized = true;

    return { success: true, data: frame.data };
  }

  /**
   * Apply a patch frame to the current state.
   */
  applyPatch(frame: PatchFrame): {
    success: boolean;
    appliedPatches: number;
    skippedPatches: number;
    totalPatches: number;
    error?: string;
  } {
    const patches = frame.patches ?? [];
    const total = patches.length;

    if (!this.initialized) {
      return {
        success: false,
        appliedPatches: 0,
        skippedPatches: total,
        totalPatches: total,
        error: 'Reconstructor not initialized — call processSkeleton first'
      };
    }

    let applied = 0;
    let skipped = 0;

    for (const patch of patches) {
      if (!patch.path || !patch.path.startsWith('$.')) {
        skipped++;
        continue;
      }

      try {
        switch (patch.operation) {
          case 'set':
            this.setPatchValue(this.state, patch.path, patch.value);
            break;
          case 'append':
            this.appendPatchValue(this.state, patch.path, patch.value);
            break;
          case 'merge':
            this.mergePatchValue(this.state, patch.path, patch.value);
            break;
          case 'delete':
            this.deletePatchValue(this.state, patch.path);
            break;
          default:
            skipped++;
            continue;
        }
        applied++;
        this.patchesApplied++;
      } catch {
        skipped++;
      }
    }

    return {
      success: applied > 0 || skipped === 0,
      appliedPatches: applied,
      skippedPatches: skipped,
      totalPatches: total
    };
  }

  /** Returns a deep clone of the current state, or null if not initialized. */
  getCurrentState(): any {
    if (this.state === null) return null;
    return this.deepClone(this.state);
  }

  /** Whether the stream has been marked complete. */
  isComplete(): boolean {
    return this.complete;
  }

  /** Mark the stream as complete. */
  markComplete(): void {
    this.complete = true;
  }

  /** Metadata about the current reconstructor state. */
  getMetadata(): {
    isInitialized: boolean;
    isComplete: boolean;
    patchesApplied: number;
    memoryUsage: number;
    estimatedSize: number;
  } {
    const size = this.state !== null ? estimateObjectSize(this.state) : 0;
    return {
      isInitialized: this.initialized,
      isComplete: this.complete,
      patchesApplied: this.patchesApplied,
      memoryUsage: size,
      estimatedSize: size
    };
  }

  /** Reset all state. */
  reset(): void {
    this.state = null;
    this.initialized = false;
    this.complete = false;
    this.patchesApplied = 0;
  }

  // Path resolution helpers

  private static readonly UNSAFE_KEYS = new Set(['__proto__', 'constructor', 'prototype']);

  private static assertSafeKey(key: string): void {
    if (JsonReconstructor.UNSAFE_KEYS.has(key)) {
      throw new Error(`Unsafe path segment: ${key}`);
    }
  }

  private setPatchValue(obj: any, path: JsonPath, value: any): void {
    const { parent, key } = this.resolvePath(obj, path);
    Object.defineProperty(parent, key, { value, writable: true, enumerable: true, configurable: true });
  }

  private appendPatchValue(obj: any, path: JsonPath, value: any): void {
    const { parent, key } = this.resolvePath(obj, path);
    if (!Array.isArray(parent[key])) {
      Object.defineProperty(parent, key, { value: [], writable: true, enumerable: true, configurable: true });
    }
    if (Array.isArray(value)) {
      parent[key].push(...value);
    } else {
      parent[key].push(value);
    }
  }

  private mergePatchValue(obj: any, path: JsonPath, value: any): void {
    const { parent, key } = this.resolvePath(obj, path);
    const merged =
      typeof parent[key] === 'object' && typeof value === 'object' &&
      !Array.isArray(parent[key]) && !Array.isArray(value)
        ? { ...parent[key], ...value }
        : value;
    Object.defineProperty(parent, key, { value: merged, writable: true, enumerable: true, configurable: true });
  }

  private deletePatchValue(obj: any, path: JsonPath): void {
    const { parent, key } = this.resolvePath(obj, path);
    if (Array.isArray(parent)) {
      const index = parseInt(key, 10);
      if (!isNaN(index) && index >= 0 && index < parent.length) {
        parent.splice(index, 1);
      }
    } else if (key !== '__proto__' && key !== 'constructor' && key !== 'prototype') {
      // eslint-disable-next-line @typescript-eslint/no-dynamic-delete
      delete parent[key];
    }
  }

  private resolvePath(obj: any, path: JsonPath): { parent: any; key: string } {
    const pathSegments = path.slice(2).split('.');
    let current = obj;

    for (let i = 0; i < pathSegments.length - 1; i++) {
      const segment = pathSegments[i];
      const { key, index } = this.parseSegment(segment);
      JsonReconstructor.assertSafeKey(key);

      if (index !== null) {
        current = current[key];
        current = current[index];
      } else {
        if (!(key in current)) {
          Object.defineProperty(current, key, { value: {}, writable: true, enumerable: true, configurable: true });
        }
        current = current[key];
      }
    }

    const lastSegment = pathSegments[pathSegments.length - 1];
    const { key, index } = this.parseSegment(lastSegment);
    JsonReconstructor.assertSafeKey(key);

    if (index !== null) {
      if (!Array.isArray(current[key])) {
        Object.defineProperty(current, key, { value: [], writable: true, enumerable: true, configurable: true });
      }
      return { parent: current[key], key: index.toString() };
    }
    return { parent: current, key };
  }

  private parseSegment(segment: string): { key: string; index: number | null } {
    const arrayMatch = segment.match(/^([^[\]]+)\[(\d+)\]$/);
    if (arrayMatch) {
      return { key: arrayMatch[1], index: parseInt(arrayMatch[2], 10) };
    }
    return { key: segment, index: null };
  }

  private deepClone<T>(obj: T): T {
    if (obj === null || typeof obj !== 'object') return obj;
    if (obj instanceof Date) return new Date((obj as any).getTime()) as T;
    if (Array.isArray(obj)) return (obj as any[]).map(item => this.deepClone(item)) as T;
    const cloned = {} as T;
    for (const key in obj) {
      if (Object.prototype.hasOwnProperty.call(obj, key)) {
        cloned[key] = this.deepClone((obj as any)[key]);
      }
    }
    return cloned;
  }
}
