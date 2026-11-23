const RELATIVE_SEGMENTS: ReadonlySet<string> = new Set(['.', '..']);

function isHiddenSegment(segment: string): boolean {
  return segment.startsWith('.') && !RELATIVE_SEGMENTS.has(segment);
}

/** Return true when any parent directory segment is a hidden dot-directory, preserving root dotfiles. */
export function isHiddenDirectoryPath(path: string): boolean {
  const normalized = path.replaceAll('\\', '/');
  const segments = normalized.split('/');
  const parents = segments.slice(0, -1);

  return parents.some((segment) => isHiddenSegment(segment));
}
