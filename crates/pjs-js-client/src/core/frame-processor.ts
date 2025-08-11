/**
 * Frame Processor - Handles validation and processing of PJS frames
 * 
 * This module is responsible for validating incoming frames,
 * filtering by priority, and preparing them for JSON reconstruction.
 */

import {
  Frame,
  FrameType,
  Priority,
  SkeletonFrame,
  PatchFrame,
  CompleteFrame,
  PatchOperation,
  PJSError,
  PJSErrorType,
  JsonPath
} from '../types/index.js';

export interface FrameProcessorConfig {
  debug?: boolean;
  priorityThreshold: Priority;
  strictValidation?: boolean;
}

/**
 * Processes and validates PJS frames according to protocol specification
 */
export class FrameProcessor {
  private config: FrameProcessorConfig;
  private frameCount = 0;
  private receivedFrameTypes = new Set<FrameType>();
  
  constructor(config: FrameProcessorConfig) {
    this.config = {
      strictValidation: true,
      ...config
    };
  }

  /**
   * Process and validate an incoming frame
   */
  processFrame(frame: any): Frame {
    this.frameCount++;
    
    try {
      // Basic structure validation
      const validatedFrame = this.validateFrameStructure(frame);
      
      // Priority filtering
      if (validatedFrame.priority < this.config.priorityThreshold) {
        if (this.config.debug) {
          console.log(`[PJS] Filtering frame with priority ${validatedFrame.priority} (threshold: ${this.config.priorityThreshold})`);
        }
        throw new PJSError(
          PJSErrorType.ValidationError,
          `Frame priority ${validatedFrame.priority} below threshold ${this.config.priorityThreshold}`
        );
      }
      
      // Protocol validation
      this.validateProtocolOrder(validatedFrame);
      
      // Type-specific validation
      this.validateFrameContent(validatedFrame);
      
      this.receivedFrameTypes.add(validatedFrame.type);
      
      if (this.config.debug) {
        console.log(`[PJS] Processed frame ${this.frameCount}:`, {
          type: validatedFrame.type,
          priority: validatedFrame.priority,
          contentSize: this.getFrameContentSize(validatedFrame)
        });
      }
      
      return validatedFrame;
      
    } catch (error) {
      if (error instanceof PJSError) {
        throw error;
      }
      
      throw new PJSError(
        PJSErrorType.ParseError,
        `Failed to process frame ${this.frameCount}`,
        { frame, frameCount: this.frameCount },
        error as Error
      );
    }
  }

  /**
   * Reset processor state for new stream
   */
  reset(): void {
    this.frameCount = 0;
    this.receivedFrameTypes.clear();
  }

  /**
   * Get processing statistics
   */
  getStats() {
    return {
      frameCount: this.frameCount,
      receivedFrameTypes: Array.from(this.receivedFrameTypes),
      priorityThreshold: this.config.priorityThreshold
    };
  }

  // Private validation methods

  private validateFrameStructure(frame: any): Frame {
    if (!frame || typeof frame !== 'object') {
      throw new PJSError(
        PJSErrorType.ValidationError,
        'Frame must be an object'
      );
    }

    // Validate required fields
    if (!frame.type) {
      throw new PJSError(
        PJSErrorType.ValidationError,
        'Frame must have a type field'
      );
    }

    if (!Object.values(FrameType).includes(frame.type)) {
      throw new PJSError(
        PJSErrorType.ValidationError,
        `Invalid frame type: ${frame.type}`
      );
    }

    if (typeof frame.priority !== 'number') {
      throw new PJSError(
        PJSErrorType.ValidationError,
        'Frame must have a numeric priority field'
      );
    }

    // Normalize priority to known values
    const priority = this.normalizePriority(frame.priority);

    // Add timestamp if not present
    const timestamp = frame.timestamp ?? Date.now();

    return {
      ...frame,
      priority,
      timestamp
    } as Frame;
  }

  private validateProtocolOrder(frame: Frame): void {
    if (!this.config.strictValidation) return;

    switch (frame.type) {
      case FrameType.Skeleton:
        // Skeleton should be first frame, but multiple skeletons are allowed for different data sections
        break;
        
      case FrameType.Patch:
        // Patch frames can only come after skeleton
        if (!this.receivedFrameTypes.has(FrameType.Skeleton)) {
          throw new PJSError(
            PJSErrorType.ProtocolError,
            'Patch frame received before skeleton frame'
          );
        }
        break;
        
      case FrameType.Complete:
        // Complete frame should come after at least a skeleton
        if (!this.receivedFrameTypes.has(FrameType.Skeleton)) {
          throw new PJSError(
            PJSErrorType.ProtocolError,
            'Complete frame received without skeleton frame'
          );
        }
        break;
    }
  }

  private validateFrameContent(frame: Frame): void {
    switch (frame.type) {
      case FrameType.Skeleton:
        this.validateSkeletonFrame(frame as SkeletonFrame);
        break;
        
      case FrameType.Patch:
        this.validatePatchFrame(frame as PatchFrame);
        break;
        
      case FrameType.Complete:
        this.validateCompleteFrame(frame as CompleteFrame);
        break;
    }
  }

  private validateSkeletonFrame(frame: SkeletonFrame): void {
    if (frame.data === undefined) {
      throw new PJSError(
        PJSErrorType.ValidationError,
        'Skeleton frame must have data field'
      );
    }

    if (frame.complete !== false && frame.complete !== undefined) {
      throw new PJSError(
        PJSErrorType.ValidationError,
        'Skeleton frame complete field must be false or undefined'
      );
    }
  }

  private validatePatchFrame(frame: PatchFrame): void {
    if (!Array.isArray(frame.patches)) {
      throw new PJSError(
        PJSErrorType.ValidationError,
        'Patch frame must have patches array'
      );
    }

    if (frame.patches.length === 0) {
      throw new PJSError(
        PJSErrorType.ValidationError,
        'Patch frame must have at least one patch operation'
      );
    }

    // Validate each patch operation
    for (let i = 0; i < frame.patches.length; i++) {
      this.validatePatchOperation(frame.patches[i], i);
    }
  }

  private validateCompleteFrame(frame: CompleteFrame): void {
    // Complete frames are minimal, just validate optional fields
    if (frame.total_frames !== undefined && typeof frame.total_frames !== 'number') {
      throw new PJSError(
        PJSErrorType.ValidationError,
        'Complete frame total_frames must be a number if present'
      );
    }

    if (frame.checksum !== undefined && typeof frame.checksum !== 'string') {
      throw new PJSError(
        PJSErrorType.ValidationError,
        'Complete frame checksum must be a string if present'
      );
    }
  }

  private validatePatchOperation(patch: PatchOperation, index: number): void {
    if (!patch || typeof patch !== 'object') {
      throw new PJSError(
        PJSErrorType.ValidationError,
        `Patch operation ${index} must be an object`
      );
    }

    if (!patch.path || typeof patch.path !== 'string') {
      throw new PJSError(
        PJSErrorType.ValidationError,
        `Patch operation ${index} must have a valid path`
      );
    }

    if (!this.isValidJsonPath(patch.path)) {
      throw new PJSError(
        PJSErrorType.ValidationError,
        `Patch operation ${index} has invalid JSON path: ${patch.path}`
      );
    }

    const validOperations = ['set', 'append', 'merge', 'delete'];
    if (!patch.operation || !validOperations.includes(patch.operation)) {
      throw new PJSError(
        PJSErrorType.ValidationError,
        `Patch operation ${index} has invalid operation: ${patch.operation}`
      );
    }

    // Value is required for all operations except delete
    if (patch.operation !== 'delete' && patch.value === undefined) {
      throw new PJSError(
        PJSErrorType.ValidationError,
        `Patch operation ${index} with operation '${patch.operation}' must have a value`
      );
    }
  }

  private isValidJsonPath(path: JsonPath): boolean {
    // Basic JSON path validation
    if (!path.startsWith('$')) {
      return false;
    }

    // Allow common patterns: $.field, $.field[0], $.field.subfield
    const pathRegex = /^\$(\.[a-zA-Z_][a-zA-Z0-9_]*(\[\d+\])?)*$/;
    return pathRegex.test(path);
  }

  private normalizePriority(priority: number): Priority {
    // Find closest Priority enum value
    const priorities = Object.values(Priority)
      .filter(p => typeof p === 'number') as number[];
    
    const closest = priorities.reduce((prev, curr) => 
      Math.abs(curr - priority) < Math.abs(prev - priority) ? curr : prev
    );
    
    return closest as Priority;
  }

  private getFrameContentSize(frame: Frame): number {
    try {
      return JSON.stringify(frame).length;
    } catch {
      return 0;
    }
  }
}