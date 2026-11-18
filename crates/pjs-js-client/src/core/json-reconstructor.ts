/**
 * JSON Reconstructor - Progressive JSON reconstruction from PJS frames
 *
 * Reconstructs complete JSON objects from skeleton and patch frames,
 * applying operations in priority order.
 */

import {
  PatchFrame,
  PatchOperationKind,
  SkeletonFrame
} from '../types/index.js';

import { estimateObjectSize } from '../utils/index.js';
import { JsonPathSegment, tokenizeJsonPath } from './json-path.js';

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

    if (frame.payload === undefined) {
      return { success: false, error: 'Skeleton frame missing payload field' };
    }

    this.state = this.deepClone(frame.payload);
    this.initialized = true;

    return { success: true, data: frame.payload };
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
    const patches = frame.payload?.patches ?? [];
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
      const parsed = typeof patch.path === 'string' ? tokenizeJsonPath(patch.path) : null;
      if (parsed === null || !parsed.ok) {
        skipped++;
        continue;
      }

      if (parsed.segments.length === 0) {
        if (this.applyRootPatch(patch.operation, patch.value)) {
          applied++;
          this.patchesApplied++;
        } else {
          skipped++;
        }
        continue;
      }

      try {
        switch (patch.operation) {
          case 'set':
            this.setPatchValue(parsed.segments, patch.value);
            break;
          case 'append':
            this.appendPatchValue(parsed.segments, patch.value);
            break;
          case 'merge':
            this.mergePatchValue(parsed.segments, patch.value);
            break;
          case 'delete':
            this.deletePatchValue(parsed.segments);
            break;
          case 'append_str':
            this.appendStrPatchValue(parsed.segments, patch.value);
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

  /** Applies a patch addressed at the root path `$`; `delete` is not meaningful there. */
  private applyRootPatch(operation: PatchOperationKind, value: unknown): boolean {
    switch (operation) {
      case 'append':
        if (!Array.isArray(this.state)) return false;
        JsonReconstructor.pushValue(this.state, value);
        return true;
      case 'append_str':
        if (typeof this.state !== 'string' || typeof value !== 'string') return false;
        this.state += value;
        return true;
      case 'set':
        this.state = this.deepClone(value);
        return true;
      case 'merge':
        if (JsonReconstructor.isPlainObject(this.state) && JsonReconstructor.isPlainObject(value)) {
          const target = this.state as Record<string, unknown>;
          const source = value as Record<string, unknown>;
          for (const key of Object.keys(source)) {
            Object.defineProperty(target, key, { value: this.deepClone(source[key]), writable: true, enumerable: true, configurable: true });
          }
        } else {
          this.state = this.deepClone(value);
        }
        return true;
      default:
        return false;
    }
  }

  private static isPlainObject(value: unknown): boolean {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
  }

  // Path resolution helpers

  private static ownValue(container: any, key: string | number): any {
    return Object.prototype.hasOwnProperty.call(container, key) ? container[key] : undefined;
  }

  private static defineOwn(container: any, key: string | number, value: unknown): void {
    Object.defineProperty(container, key, { value, writable: true, enumerable: true, configurable: true });
  }

  private setPatchValue(segments: readonly JsonPathSegment[], value: any): void {
    const { parent, key } = this.resolveSlot(segments);
    JsonReconstructor.defineOwn(parent, key, value);
  }

  private appendPatchValue(segments: readonly JsonPathSegment[], value: any): void {
    const { parent, key } = this.resolveSlot(segments);
    let target = JsonReconstructor.ownValue(parent, key);
    if (!Array.isArray(target)) {
      target = [];
      JsonReconstructor.defineOwn(parent, key, target);
    }
    JsonReconstructor.pushValue(target, value);
  }

  private appendStrPatchValue(segments: readonly JsonPathSegment[], value: unknown): void {
    const { parent, key } = this.resolveSlot(segments);
    const existing = JsonReconstructor.ownValue(parent, key);
    if (typeof existing !== 'string' || typeof value !== 'string') {
      throw new Error('append_str target is not a string');
    }
    JsonReconstructor.defineOwn(parent, key, existing + value);
  }

  /** Pushes the elements of an array value, or a scalar value once. */
  private static pushValue(target: unknown[], value: unknown): void {
    if (Array.isArray(value)) {
      // A loop, not push(...value): spreading a very large array overflows the call stack.
      for (const element of value) target.push(element);
    } else {
      target.push(value);
    }
  }

  private mergePatchValue(segments: readonly JsonPathSegment[], value: any): void {
    const { parent, key } = this.resolveSlot(segments);
    const existing = JsonReconstructor.ownValue(parent, key);
    let merged = value;
    if (
      typeof existing === 'object' && typeof value === 'object' &&
      !Array.isArray(existing) && !Array.isArray(value)
    ) {
      merged = { ...existing, ...value };
    }
    JsonReconstructor.defineOwn(parent, key, merged);
  }

  private deletePatchValue(segments: readonly JsonPathSegment[]): void {
    const { parent, key } = this.resolveSlot(segments);
    if (Array.isArray(parent)) {
      if (typeof key === 'number') parent.splice(key, 1);
    } else if (Object.prototype.hasOwnProperty.call(parent, key)) {
      // eslint-disable-next-line @typescript-eslint/no-dynamic-delete
      delete parent[key];
    }
  }

  /**
   * Resolves the container and key addressed by a non-empty segment list.
   * Indices must already exist; only object keys followed by a key are created.
   */
  private resolveSlot(segments: readonly JsonPathSegment[]): { parent: any; key: string | number } {
    let current: any = this.state;
    const last = segments.length - 1;

    for (let i = 0; i < last; i++) {
      const segment = segments[i];
      if (segment.kind === 'index') {
        current = JsonReconstructor.elementAt(current, segment.index);
        continue;
      }
      if (!JsonReconstructor.isPlainObject(current)) {
        throw new Error(`Path segment is not an object: ${segment.key}`);
      }
      if (!Object.prototype.hasOwnProperty.call(current, segment.key)) {
        if (segments[i + 1].kind === 'index') {
          throw new Error(`Path segment not found: ${segment.key}`);
        }
        JsonReconstructor.defineOwn(current, segment.key, Object.create(null));
      }
      current = current[segment.key];
    }

    const tail = segments[last];
    if (tail.kind === 'index') {
      JsonReconstructor.elementAt(current, tail.index);
      return { parent: current, key: tail.index };
    }
    if (!JsonReconstructor.isPlainObject(current)) {
      throw new Error(`Path segment is not an object: ${tail.key}`);
    }
    return { parent: current, key: tail.key };
  }

  private static elementAt(container: unknown, index: number): any {
    if (!Array.isArray(container) || index >= container.length) {
      throw new Error(`Index out of range: ${index}`);
    }
    return container[index];
  }

  private deepClone<T>(obj: T): T {
    if (obj === null || typeof obj !== 'object') return obj;
    if (obj instanceof Date) return new Date((obj as any).getTime()) as T;
    if (Array.isArray(obj)) return (obj as any[]).map(item => this.deepClone(item)) as T;
    // Object.create(null) produces a prototype-free object, preventing prototype
    // pollution if user-controlled keys like __proto__ appear in the data.
    const cloned: Record<string, unknown> = Object.create(null);
    for (const key of Object.keys(obj)) {
      cloned[key] = this.deepClone((obj as any)[key]);
    }
    return cloned as T;
  }
}
