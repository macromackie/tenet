import { createHash } from 'node:crypto';

/** Hash the stable serialization of a surface, an observation or a file inventory. */
export function hashSource(data: string): string {
  const sha = createHash('sha256');

  sha.update(data);

  return sha.digest('hex');
}
