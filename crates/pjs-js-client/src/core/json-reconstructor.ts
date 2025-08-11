/**
 * JSON Reconstructor - Progressive JSON reconstruction from PJS frames
 * 
 * This module handles the reconstruction of complete JSON objects
 * from skeleton and patch frames, applying operations in the correct order.
 */

import {
  JsonPath,
  PatchOperation,
  PJSError,
  PJSErrorType,
  MemoryStats
} from '../types/index.js';

export interface JsonReconstructorConfig {
  bufferSize: number;
  debug?: boolean;
  trackMemoryUsage?: boolean;
}

/**
 * Reconstructs JSON objects from PJS frames using efficient patching
 */
export class JsonReconstructor {
  private config: JsonReconstructorConfig;
  private memoryStats: MemoryStats;
  private operationCount = 0;
  
  constructor(config: JsonReconstructorConfig) {
    this.config = {
      trackMemoryUsage: true,
      ...config
    };
    
    this.memoryStats = {
      totalAllocated: 0,
      totalReferenced: 0,
      efficiency: 0,
      peakUsage: 0
    };
  }

  /**
   * Apply skeleton frame to initialize JSON structure
   */
  applySkeleton<T>(skeletonData: any): T {
    try {
      this.operationCount++;
      
      if (this.config.trackMemoryUsage) {
        const size = this.estimateObjectSize(skeletonData);
        this.memoryStats.totalAllocated += size;
        this.memoryStats.totalReferenced += size;
        this.updatePeakUsage();
      }
      
      // Deep clone the skeleton to avoid mutations
      const result = this.deepClone(skeletonData);
      
      if (this.config.debug) {
        console.log('[PJS] Applied skeleton:', {
          operationCount: this.operationCount,
          dataSize: this.estimateObjectSize(result)
        });
      }
      
      return result;
      
    } catch (error) {
      throw new PJSError(
        PJSErrorType.ParseError,
        'Failed to apply skeleton data',
        { skeletonData },
        error as Error
      );
    }
  }

  /**
   * Apply a patch operation to existing JSON structure
   */
  applyPatch<T>(data: T, patch: PatchOperation): T {
    try {
      this.operationCount++;
      
      const result = this.cloneForPatch(data);
      
      switch (patch.operation) {
        case 'set':
          this.setPatchValue(result, patch.path, patch.value);
          break;
          
        case 'append':
          this.appendPatchValue(result, patch.path, patch.value);
          break;
          
        case 'merge':
          this.mergePatchValue(result, patch.path, patch.value);
          break;
          
        case 'delete':
          this.deletePatchValue(result, patch.path);
          break;
          
        default:
          throw new PJSError(
            PJSErrorType.ValidationError,
            `Unsupported patch operation: ${patch.operation}`
          );
      }
      
      if (this.config.trackMemoryUsage) {
        const size = this.estimateObjectSize(result);
        this.memoryStats.totalAllocated += size;
        this.updateEfficiency();
        this.updatePeakUsage();
      }
      
      if (this.config.debug) {
        console.log(`[PJS] Applied ${patch.operation} patch:`, {
          path: patch.path,
          operationCount: this.operationCount,
          hasValue: patch.value !== undefined
        });
      }
      
      return result;
      
    } catch (error) {
      if (error instanceof PJSError) {
        throw error;
      }
      
      throw new PJSError(
        PJSErrorType.ParseError,
        `Failed to apply patch operation: ${patch.operation}`,
        { patch, data },
        error as Error
      );
    }
  }

  /**
   * Get current memory usage statistics
   */
  getMemoryStats(): MemoryStats {
    return { ...this.memoryStats };
  }

  /**
   * Reset reconstructor state
   */
  reset(): void {
    this.operationCount = 0;
    this.memoryStats = {
      totalAllocated: 0,
      totalReferenced: 0,
      efficiency: 0,
      peakUsage: 0
    };
  }

  // Private methods for path operations

  private setPatchValue(obj: any, path: JsonPath, value: any): void {
    const { parent, key } = this.resolvePath(obj, path);
    parent[key] = value;
  }

  private appendPatchValue(obj: any, path: JsonPath, value: any): void {
    const { parent, key } = this.resolvePath(obj, path);
    
    if (!Array.isArray(parent[key])) {
      parent[key] = [];
    }
    
    if (Array.isArray(value)) {
      parent[key].push(...value);
    } else {
      parent[key].push(value);
    }
  }

  private mergePatchValue(obj: any, path: JsonPath, value: any): void {
    const { parent, key } = this.resolvePath(obj, path);
    
    if (typeof parent[key] === 'object' && typeof value === 'object' && 
        !Array.isArray(parent[key]) && !Array.isArray(value)) {
      parent[key] = { ...parent[key], ...value };
    } else {
      parent[key] = value;
    }
  }

  private deletePatchValue(obj: any, path: JsonPath): void {
    const { parent, key } = this.resolvePath(obj, path);
    
    if (Array.isArray(parent)) {
      const index = parseInt(key, 10);
      if (!isNaN(index) && index >= 0 && index < parent.length) {
        parent.splice(index, 1);
      }
    } else {
      delete parent[key];
    }
  }

  private resolvePath(obj: any, path: JsonPath): { parent: any; key: string } {
    if (!path.startsWith('$.')) {
      throw new PJSError(
        PJSErrorType.ValidationError,
        `Invalid JSON path: ${path}`
      );
    }

    // Remove the '$.' prefix
    const pathSegments = path.slice(2).split('.');
    let current = obj;
    
    // Navigate to parent object
    for (let i = 0; i < pathSegments.length - 1; i++) {
      const segment = pathSegments[i];
      const { key, index } = this.parseSegment(segment);
      
      if (index !== null) {
        // Handle array access
        current = current[key];
        if (!Array.isArray(current)) {
          throw new PJSError(
            PJSErrorType.ParseError,
            `Path segment ${segment} expects array but found ${typeof current}`
          );
        }
        current = current[index];
      } else {
        // Handle object access
        if (!(key in current)) {
          current[key] = {};
        }
        current = current[key];
      }
    }
    
    const lastSegment = pathSegments[pathSegments.length - 1];
    const { key, index } = this.parseSegment(lastSegment);
    
    if (index !== null) {
      // Last segment is array access
      const parent = current[key];
      if (!Array.isArray(parent)) {
        current[key] = [];
      }
      return { parent: current[key], key: index.toString() };
    } else {
      // Last segment is object key
      return { parent: current, key };
    }
  }

  private parseSegment(segment: string): { key: string; index: number | null } {
    const arrayMatch = segment.match(/^([^[\]]+)\[(\d+)\]$/);
    if (arrayMatch) {
      return {
        key: arrayMatch[1],
        index: parseInt(arrayMatch[2], 10)
      };
    }
    return { key: segment, index: null };
  }

  // Utility methods

  private deepClone<T>(obj: T): T {
    if (obj === null || typeof obj !== 'object') {
      return obj;
    }
    
    if (obj instanceof Date) {
      return new Date(obj.getTime()) as T;
    }
    
    if (Array.isArray(obj)) {
      return obj.map(item => this.deepClone(item)) as T;
    }
    
    const cloned = {} as T;
    for (const key in obj) {
      if (obj.hasOwnProperty(key)) {
        cloned[key] = this.deepClone(obj[key]);
      }
    }
    
    return cloned;
  }

  private cloneForPatch<T>(obj: T): T {
    // For performance, we can use a shallow clone for most patch operations
    // and only deep clone when necessary
    return JSON.parse(JSON.stringify(obj));
  }

  private estimateObjectSize(obj: any): number {
    try {
      return JSON.stringify(obj).length * 2; // Rough estimate including UTF-16 encoding
    } catch {
      return 0;
    }
  }

  private updateEfficiency(): void {
    if (this.memoryStats.totalAllocated > 0) {
      this.memoryStats.efficiency = 
        this.memoryStats.totalReferenced / this.memoryStats.totalAllocated;
    }
  }

  private updatePeakUsage(): void {
    this.memoryStats.peakUsage = Math.max(
      this.memoryStats.peakUsage,
      this.memoryStats.totalAllocated
    );
  }
}