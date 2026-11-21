import { dirname, posix } from 'node:path';
import { Option } from 'effect';

const ROOT_SEGMENTS: ReadonlySet<string> = new Set(['.', '/']);

type PythonPathsInput = Readonly<{
  files: ReadonlyMap<string, string>;
  root: string;
}>;

type SourceFolderInput = Readonly<{
  files: ReadonlyMap<string, string>;
  path: string;
}>;

function isPackageDirectory({ files, path }: SourceFolderInput): boolean {
  const initPath = posix.join(path, '__init__.py');

  return files.has(initPath);
}

function readSourceFolder({ files, path }: SourceFolderInput): Option.Option<string> {
  if (path.endsWith('/__init__.py')) {
    const pkg = dirname(path);
    const parent = dirname(pkg);

    if (!ROOT_SEGMENTS.has(parent) && !isPackageDirectory({ files, path: parent })) {
      return Option.some(parent);
    }

    return Option.none();
  }

  if (path.startsWith('src/') && path.endsWith('.py')) {
    return Option.some('src');
  }

  return Option.none();
}

/** Build a PYTHONPATH covering the staged checkout root and any nested Python source roots. */
export function readPythonPaths({ files, root }: PythonPathsInput): string {
  const extraFolders = new Set<string>();

  for (const path of files.keys()) {
    const folder = readSourceFolder({ files, path });

    if (Option.isSome(folder)) {
      extraFolders.add(folder.value);
    }
  }

  const sorted = [...extraFolders].toSorted();
  const resolved = sorted.map((folder) => posix.join(root, folder));

  return [root, ...resolved].join(':');
}
