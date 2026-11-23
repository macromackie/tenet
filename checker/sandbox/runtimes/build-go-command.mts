import type { ProjectDeps } from '../project-deps.mts';

type GoCommandInput = Readonly<{
  binary: string;
  deps: ProjectDeps;
  files: ReadonlyMap<string, string>;
  isTestProbe?: boolean;
  output: string;
  probeDir: string;
}>;

type HasVendorInput = Readonly<{
  deps: ProjectDeps;
  files: ReadonlyMap<string, string>;
}>;

const GO_SCRIPT =
  'go="$1"; out="$2"; probe="$3"; mode="$4"; "$go" build -mod="$mode" -o "$out" "./$probe" && exec "$out"';

const GO_TEST_SCRIPT =
  'go="$1"; pkg="$2"; mode="$3"; exec "$go" test -mod="$mode" -count=1 -run "^TestTenetObserve$" -v "$pkg"';

function readGoModMode({ deps, files }: HasVendorInput): 'readonly' | 'vendor' {
  if (deps.goVendor !== undefined || [...files.keys()].some((path) => path.startsWith('vendor/'))) {
    return 'vendor';
  }

  return 'readonly';
}

function formatGoTestPackage(probeDir: string): string {
  if (probeDir === '.' || probeDir.length === 0) {
    return '.';
  }

  return `./${probeDir}`;
}

/** Build a sandboxed Go compile-and-run command targeting the isolated probe package or same-package test probe. */
export function buildGoCommand({
  binary,
  deps,
  files,
  isTestProbe = false,
  output,
  probeDir,
}: GoCommandInput): readonly string[] {
  const mode = readGoModMode({ deps, files });

  if (isTestProbe) {
    const pkg = formatGoTestPackage(probeDir);

    return ['/bin/sh', '-c', GO_TEST_SCRIPT, 'tenet-go-test', binary, pkg, mode];
  }

  return ['/bin/sh', '-c', GO_SCRIPT, 'tenet-go', binary, output, probeDir, mode];
}
