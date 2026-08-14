/**
 * Fallback ambient type declarations for the optional `pjs-wasm` package.
 *
 * `pjs-wasm` is an `optionalDependencies` file-link to `../pjs-wasm/pkg`,
 * the wasm-pack build output, which is absent until that crate is built
 * (e.g. in CI jobs that don't build wasm). TypeScript only falls back to
 * this declaration when the real package isn't resolvable on disk — when
 * it is, the real generated `pjs_wasm.d.ts` takes precedence and this file
 * has no effect. Mirrors the subset of that generated file actually used
 * by this package; keep in sync if the wasm bindings' public API changes.
 */
declare module 'pjs-wasm' {
  export interface FrameData {
    type: string;
    sequence: number;
    priority: number;
    payload: string;
  }

  export interface StreamStats {
    totalFrames: number;
    patchFrames: number;
    bytesProcessed: number;
    durationMs: number;
  }

  export class PjsParser {
    constructor();
    free(): void;
    parse(json_str: string): any;
    generateFrames(json_str: string, min_priority: number): any;
    static version(): string;
    static withConfig(config_builder: unknown): PjsParser;
    static withSecurityConfig(security_config: unknown): PjsParser;
  }

  export class PriorityStream {
    constructor();
    free(): void;
    onComplete(callback: (stats: StreamStats) => void): void;
    onError(callback: (error: string) => void): void;
    onFrame(callback: (frame: FrameData) => void): void;
    setMinPriority(priority: number): void;
    setSecurityConfig(config: unknown): void;
    start(json_str: string): void;
    static withConfig(config_builder: unknown): PriorityStream;
  }

  export function version(): string;

  export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

  export default function init(
    module_or_path?: InitInput | Promise<InitInput>
  ): Promise<unknown>;
}
