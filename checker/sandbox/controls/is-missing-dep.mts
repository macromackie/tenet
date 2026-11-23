const UNRECOVERABLE_DEP_PATTERNS = [
  /\(missing external package "/v,
  /target\/debug\/deps/v,
  /go mod vendor/v,
  /missing go\.sum entry/v,
] as const;

/** Return true when a candidate failure message indicates an unrecoverable missing external dependency. */
export function isMissingExternalDepMessage(message: string): boolean {
  return UNRECOVERABLE_DEP_PATTERNS.some((pattern) => pattern.test(message));
}
