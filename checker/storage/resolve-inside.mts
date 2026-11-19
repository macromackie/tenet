import { isAbsolute, join, resolve, sep } from 'node:path';
import { CheckerError } from '../checker-error.mts';

export type LocationInput = Readonly<{ path: string; root: string }>;

/** Resolve a checkout-relative record or source path inside the selected root. */
export function resolveInside({ root, path }: LocationInput): string {
  const target = resolve(root, path);
  const prefix = join(root, sep);
  const components = path.split(sep);

  if (isAbsolute(path) || components.includes('..') || !target.startsWith(prefix)) {
    throw new CheckerError({ message: 'input falls outside the checkout.' });
  }

  return target;
}
