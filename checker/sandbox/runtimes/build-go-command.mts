import type { ProjectDeps } from '../project-deps.mts';

type GoCommandInput = Readonly<{
  binary: string;
  deps: ProjectDeps;
  files: ReadonlyMap<string, string>;
  output: string;
  probeDir: string;
}>;

type HasVendorInput = Readonly<{
  deps: ProjectDeps;
  files: ReadonlyMap<string, string>;
}>;

const GO_SCRIPT =
  'go="$1"; out="$2"; probe="$3"; mode="$4"; "$go" build -mod="$mode" -o "$out" "./$probe" && exec "$out"';

function readGoModMode({ deps, files }: HasVendorInput): 'readonly' | 'vendor' {
  if (deps.goVendor !== undefined || [...files.keys()].some((path) => path.startsWith('vendor/'))) {
    return 'vendor';
  }

  return 'readonly';
}

/** Build a sandboxed Go compile-and-run command targeting the isolated probe package. */
export function buildGoCommand({
  binary,
  deps,
  files,
  output,
  probeDir,
}: GoCommandInput): readonly string[] {
  const mode = readGoModMode({ deps, files });

  return ['/bin/sh', '-c', GO_SCRIPT, 'tenet-go', binary, output, probeDir, mode];
}
