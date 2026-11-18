/** Narrowing helpers for parsed JSON shared by the transports. */

/** True for a non-null, non-array object. */
export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** Reads `key` from a parsed JSON object; `undefined` for anything else. */
export function field(value: unknown, key: string): unknown {
  return isRecord(value) ? value[key] : undefined;
}
