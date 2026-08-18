/**
 * Frame Processor - Validates and processes PJS frames
 *
 * Validates incoming frames against the PJS protocol specification and
 * enforces the required frame ordering and priority constraints.
 */

import {
  FrameType,
  Priority,
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
  validateFrame(frame: unknown): { isValid: boolean; errors: string[] } {
    const errors: string[] = [];

    if (!frame || typeof frame !== 'object') {
      errors.push('Frame must be an object');
      return { isValid: false, errors };
    }

    const candidate = frame as Record<string, unknown>;

    if (!candidate.frame_type) {
      errors.push('Frame missing frame_type field');
      return { isValid: false, errors };
    }

    if (!Object.values(FrameType).includes(candidate.frame_type as FrameType)) {
      errors.push(`Invalid frame type: ${candidate.frame_type}`);
      return { isValid: false, errors };
    }

    if (typeof candidate.priority !== 'number') {
      errors.push('Frame missing numeric priority field');
    } else if (candidate.priority < 0 || candidate.priority > 100) {
      errors.push('Priority must be between 0 and 100');
    }

    if (candidate.frame_type === FrameType.Skeleton) {
      if (candidate.payload === undefined) {
        errors.push('Skeleton frame must have payload field');
      }
    }

    if (candidate.frame_type === FrameType.Patch) {
      const payload = candidate.payload as Record<string, unknown> | undefined;
      if (!payload || !Array.isArray(payload.patches)) {
        errors.push('Patch frame must have payload.patches array');
      } else if (payload.patches.length === 0) {
        errors.push('Patch frame must have at least one patch operation');
      } else {
        for (let i = 0; i < payload.patches.length; i++) {
          this.validatePatchOperations(payload.patches[i], i, errors);
        }
      }
    }

    return { isValid: errors.length === 0, errors };
  }

  /**
   * Process a frame, enforcing protocol state machine and priority ordering.
   */
  processFrame(frame: unknown): {
    accepted: boolean;
    error?: { type: PJSErrorType; message: string };
  } {
    const candidate = frame as { frame_type: FrameType; priority: number };

    if (this.streamComplete) {
      return {
        accepted: false,
        error: {
          type: PJSErrorType.ProtocolViolation,
          message: 'Stream is already complete'
        }
      };
    }

    if (candidate.frame_type === FrameType.Patch && this.expectedFrameType === FrameType.Skeleton) {
      return {
        accepted: false,
        error: {
          type: PJSErrorType.ProtocolViolation,
          message: 'Expected skeleton frame first'
        }
      };
    }

    if (candidate.frame_type === FrameType.Skeleton && this.expectedFrameType === FrameType.Patch) {
      return {
        accepted: false,
        error: {
          type: PJSErrorType.ProtocolViolation,
          message: 'Duplicate skeleton frame — skeleton already received'
        }
      };
    }

    if (candidate.frame_type === FrameType.Patch) {
      if (this.lastPatchPriority !== null && candidate.priority > this.lastPatchPriority) {
        return {
          accepted: false,
          error: {
            type: PJSErrorType.ProtocolViolation,
            message: `Priority order violation: patch priority ${candidate.priority} is higher than previous patch priority ${this.lastPatchPriority}`
          }
        };
      }
    }

    // Accept frame — update state
    this.framesProcessed++;
    this.priorityDistribution[candidate.priority] = (this.priorityDistribution[candidate.priority] ?? 0) + 1;

    if (candidate.frame_type === FrameType.Skeleton) {
      this.expectedFrameType = FrameType.Patch;
    } else if (candidate.frame_type === FrameType.Patch) {
      this.lastPatchPriority = candidate.priority;
      this.patchesApplied++;
    } else if (candidate.frame_type === FrameType.Complete) {
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

  private validatePatchOperations(patch: unknown, index: number, errors: string[]): void {
    if (!patch || typeof patch !== 'object') {
      errors.push(`Patch operation ${index} must be an object`);
      return;
    }

    const candidate = patch as Record<string, unknown>;

    if (!candidate.path || typeof candidate.path !== 'string') {
      errors.push(`Patch operation ${index} must have a valid path`);
    } else if (!this.isValidJsonPath(candidate.path)) {
      errors.push(`Patch operation ${index} has invalid JSON path: ${candidate.path}`);
    }

    const validOperations = ['set', 'append', 'merge', 'delete'];
    if (typeof candidate.operation !== 'string' || !validOperations.includes(candidate.operation)) {
      errors.push(`Patch operation ${index} has invalid operation: ${candidate.operation}`);
    }
  }

  private isValidJsonPath(path: JsonPath): boolean {
    if (!path.startsWith('$')) return false;
    const pathRegex = /^\$(\.[a-zA-Z_][a-zA-Z0-9_]*(\[\d+\])?)*$/;
    return pathRegex.test(path);
  }
}
