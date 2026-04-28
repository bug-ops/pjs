/**
 * Frame Processor - Validates and processes PJS frames
 *
 * Validates incoming frames against the PJS protocol specification and
 * enforces the required frame ordering and priority constraints.
 */

import {
  Frame,
  FrameType,
  Priority,
  PatchOperation,
  PJSErrorType,
  JsonPath
} from '../types/index.js';

/**
 * Processes and validates PJS frames according to protocol specification
 */
export class FrameProcessor {
  private expectedFrameType: FrameType = FrameType.Skeleton;
  private streamComplete = false;
  private framesProcessed = 0;
  private patchesApplied = 0;
  private priorityDistribution: Record<number, number> = {};
  private lastPatchPriority: number | null = null;

  constructor() {}

  /**
   * Validate a frame without mutating state. Returns errors if any.
   */
  validateFrame(frame: any): { isValid: boolean; errors: string[] } {
    const errors: string[] = [];

    if (!frame || typeof frame !== 'object') {
      errors.push('Frame must be an object');
      return { isValid: false, errors };
    }

    if (!frame.type) {
      errors.push('Frame missing type field');
      return { isValid: false, errors };
    }

    if (!Object.values(FrameType).includes(frame.type)) {
      errors.push(`Invalid frame type: ${frame.type}`);
      return { isValid: false, errors };
    }

    if (typeof frame.priority !== 'number') {
      errors.push('Frame missing numeric priority field');
    } else if (frame.priority < 0 || frame.priority > 100) {
      errors.push('Priority must be between 0 and 100');
    }

    if (frame.type === FrameType.Skeleton) {
      if (frame.data === undefined) {
        errors.push('Skeleton frame must have data field');
      }
    }

    if (frame.type === FrameType.Patch) {
      if (!Array.isArray(frame.patches)) {
        errors.push('Patch frame must have patches array');
      } else if (frame.patches.length === 0) {
        errors.push('Patch frame must have at least one patch operation');
      } else {
        for (let i = 0; i < frame.patches.length; i++) {
          this.validatePatchOperations(frame.patches[i], i, errors);
        }
      }
    }

    return { isValid: errors.length === 0, errors };
  }

  /**
   * Process a frame, enforcing protocol state machine and priority ordering.
   */
  processFrame(frame: any): {
    accepted: boolean;
    error?: { type: PJSErrorType; message: string };
  } {
    if (this.streamComplete) {
      return {
        accepted: false,
        error: {
          type: PJSErrorType.ProtocolViolation,
          message: 'Stream is already complete'
        }
      };
    }

    if (frame.type === FrameType.Patch && this.expectedFrameType === FrameType.Skeleton) {
      return {
        accepted: false,
        error: {
          type: PJSErrorType.ProtocolViolation,
          message: 'Expected skeleton frame first'
        }
      };
    }

    if (frame.type === FrameType.Skeleton && this.expectedFrameType === FrameType.Patch) {
      return {
        accepted: false,
        error: {
          type: PJSErrorType.ProtocolViolation,
          message: 'Duplicate skeleton frame — skeleton already received'
        }
      };
    }

    if (frame.type === FrameType.Patch) {
      if (this.lastPatchPriority !== null && frame.priority > this.lastPatchPriority) {
        return {
          accepted: false,
          error: {
            type: PJSErrorType.ProtocolViolation,
            message: `Priority order violation: patch priority ${frame.priority} is higher than previous patch priority ${this.lastPatchPriority}`
          }
        };
      }
    }

    // Accept frame — update state
    this.framesProcessed++;
    this.priorityDistribution[frame.priority] = (this.priorityDistribution[frame.priority] ?? 0) + 1;

    if (frame.type === FrameType.Skeleton) {
      this.expectedFrameType = FrameType.Patch;
    } else if (frame.type === FrameType.Patch) {
      this.lastPatchPriority = frame.priority;
      this.patchesApplied++;
    } else if (frame.type === FrameType.Complete) {
      this.streamComplete = true;
    }

    return { accepted: true };
  }

  /** Returns the currently expected frame type. */
  getExpectedFrameType(): FrameType {
    return this.expectedFrameType;
  }

  /** Whether the stream has received a Complete frame. */
  isStreamComplete(): boolean {
    return this.streamComplete;
  }

  /** Processing statistics. */
  getStatistics(): {
    framesProcessed: number;
    patchesApplied: number;
    priorityDistribution: Record<Priority, number>;
  } {
    return {
      framesProcessed: this.framesProcessed,
      patchesApplied: this.patchesApplied,
      priorityDistribution: { ...this.priorityDistribution } as Record<Priority, number>
    };
  }

  /** Reset all state for a new stream. */
  reset(): void {
    this.expectedFrameType = FrameType.Skeleton;
    this.streamComplete = false;
    this.framesProcessed = 0;
    this.patchesApplied = 0;
    this.priorityDistribution = {};
    this.lastPatchPriority = null;
  }

  // Private helpers

  private validatePatchOperations(patch: PatchOperation, index: number, errors: string[]): void {
    if (!patch || typeof patch !== 'object') {
      errors.push(`Patch operation ${index} must be an object`);
      return;
    }

    if (!patch.path || typeof patch.path !== 'string') {
      errors.push(`Patch operation ${index} must have a valid path`);
    } else if (!this.isValidJsonPath(patch.path)) {
      errors.push(`Patch operation ${index} has invalid JSON path: ${patch.path}`);
    }

    const validOperations = ['set', 'append', 'merge', 'delete'];
    if (!patch.operation || !validOperations.includes(patch.operation)) {
      errors.push(`Patch operation ${index} has invalid operation: ${patch.operation}`);
    }
  }

  private isValidJsonPath(path: JsonPath): boolean {
    if (!path.startsWith('$')) return false;
    const pathRegex = /^\$(\.[a-zA-Z_][a-zA-Z0-9_]*(\[\d+\])?)*$/;
    return pathRegex.test(path);
  }
}
