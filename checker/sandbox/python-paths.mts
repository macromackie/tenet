import { dirname, posix } from 'node:path';
import { Option } from 'effect';

const ROOT_SEGMENTS: ReadonlySet<string> = new Set(['.', '/']);

type PythonPathsInput = Readonly<{
  files: ReadonlyMap<string, string>;
  root: string;
}>;

function readSourceFolder(path: string): Option.Option<string> {
  if (path.endsWith('/__init__.py')) {
    const pkg = dirname(path);
    const parent = dirname(pkg);

    if (!ROOT_SEGMENTS.has(parent)) {
      return Option.some(parent);
    }

    return Option.none();
  }

  if (path.startsWith('src/') && path.endsWith('.py')) {
    return Option.some('src');
  }

  return Option.none();
}

/** Build a PYTHONPATH covering the staged checkout root and any nested Python package roots. */
export function readPythonPaths({ files, root }: PythonPathsInput): string {
  const extraFolders = new Set<string>();

  for (const path of files.keys()) {
    const folder = readSourceFolder(path);

    if (Option.isSome(folder)) {
      extraFolders.add(folder.value);
    }
  }

  const sorted = [...extraFolders].toSorted();
  const resolved = sorted.map((folder) => posix.join(root, folder));

  return [root, ...resolved].join(':');
}
