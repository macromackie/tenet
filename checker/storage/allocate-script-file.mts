import type { Check } from '../types.d.mts';
import { isSafeScriptFile } from './is-safe-script-file.mts';
import { resolveScriptExtension } from './resolve-script-extension.mts';

const FIRST_COLLISION_SUFFIX = 2;
const MAX_SLUG_CHARS = 64;
const SLUG_CLEAN_PATTERN = /[^a-z0-9]+/gv;
const TRIM_DASH_PATTERN = /^-+|-+$/gv;

type AllocateScriptInput = Readonly<{
  check: Check;
  priorCheck?: Check;
  usedNames: ReadonlySet<string>;
}>;

type ReuseCheckInput = Readonly<{
  candidate?: string;
  extension: string;
  usedNames: ReadonlySet<string>;
}>;

type UniqueSlugInput = Readonly<{
  base: string;
  extension: string;
  usedNames: ReadonlySet<string>;
}>;

type CollisionNameInput = Readonly<{
  base: string;
  extension: string;
  suffix: number;
}>;

function formatCheckSlug(id: string): string {
  const lowered = id.trim().toLowerCase();
  const dashed = lowered.replaceAll(SLUG_CLEAN_PATTERN, '-').replaceAll(TRIM_DASH_PATTERN, '');
  const bounded = dashed.slice(0, MAX_SLUG_CHARS).replaceAll(TRIM_DASH_PATTERN, '');

  if (bounded.length > 0) {
    return bounded;
  }

  return 'check';
}

function canReuseScriptFile({ candidate, extension, usedNames }: ReuseCheckInput): boolean {
  if (candidate === undefined || !isSafeScriptFile(candidate)) {
    return false;
  }

  return candidate.endsWith(`.${extension}`) && !usedNames.has(candidate);
}

function formatCollisionName({ base, extension, suffix }: CollisionNameInput): string {
  const suffixText = String(suffix);

  return `${base}-${suffixText}.${extension}`;
}

function allocateUniqueSlug({ base, extension, usedNames }: UniqueSlugInput): string {
  const first = `${base}.${extension}`;

  if (!usedNames.has(first)) {
    return first;
  }

  let suffix = FIRST_COLLISION_SUFFIX;
  let candidate = formatCollisionName({ base, extension, suffix });

  while (usedNames.has(candidate)) {
    suffix += 1;
    candidate = formatCollisionName({ base, extension, suffix });
  }

  return candidate;
}

/** Choose a deterministic collision-free script filename inside `scripts/` for a check. */
export function allocateScriptFile({ check, priorCheck, usedNames }: AllocateScriptInput): string {
  const extension = resolveScriptExtension(check.runtime);

  if (
    priorCheck?.scriptFile !== undefined &&
    canReuseScriptFile({ candidate: priorCheck.scriptFile, extension, usedNames })
  ) {
    return priorCheck.scriptFile;
  }

  if (
    check.scriptFile !== undefined &&
    canReuseScriptFile({ candidate: check.scriptFile, extension, usedNames })
  ) {
    return check.scriptFile;
  }

  const base = formatCheckSlug(check.id);

  return allocateUniqueSlug({ base, extension, usedNames });
}
