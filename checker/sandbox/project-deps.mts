import { access, readdir, readFile, realpath, stat } from 'node:fs/promises';
import { homedir } from 'node:os';
import { delimiter, join, sep } from 'node:path';
import { env as hostEnvironment } from 'node:process';
import { Option } from 'effect';
import { CheckerError } from '../checker-error.mts';
import type { Check } from '../types.d.mts';
import { runOptional } from './run-optional.mts';
import { collectRustBuildArtifacts } from './rust-build.mts';
import { collectRustExterns, type RustExtern } from './rust-deps.mts';

export type ProjectDeps = Readonly<{
  goMod?: string;
  goModCache?: string;
  goSum?: string;
  goVendor?: string;
  nodeModules?: string;
  pythonSitePackages?: string;
  rustBuildCfgs: readonly string[];
  rustBuildOutDir?: string;
  rustDepsDir?: string;
  rustExterns: readonly RustExtern[];
}>;

type ResolveDepsInput = Readonly<{
  projectRoot?: string;
  runtime: Check['runtime'];
}>;

type SelectRuntimeDepsInput = Readonly<{
  projectRoot: string;
  runtime: Check['runtime'];
}>;

type WithinRootInput = Readonly<{ candidate?: string; root: string }>;
type SelectSafeRustInput = Readonly<{ deps: ProjectDeps; rustDepsDir?: string }>;

type FilterRootInput = Readonly<{ deps: ProjectDeps; root: string }>;

type ProjectFileInput = Readonly<{ name: string; projectRoot: string }>;

const EMPTY_DEPS = { rustBuildCfgs: [], rustExterns: [] } as const satisfies ProjectDeps;

function isBoundedDepPath(candidate: string): boolean {
  const userHome = homedir();
  const forbiddenPaths = ['/', '/Users', '/private', '/var', userHome];
  const forbiddenRoots: ReadonlySet<string> = new Set(forbiddenPaths);

  return !forbiddenRoots.has(candidate);
}

async function readCanonicalDirectory(candidate: string): Promise<string> {
  await access(candidate);

  const canonical = await realpath(candidate);
  const info = await stat(canonical);

  if (!info.isDirectory() || !isBoundedDepPath(canonical)) {
    throw new CheckerError({ message: 'dependency path is not a bounded directory' });
  }

  return canonical;
}

async function resolveDirectory(candidate: string): Promise<Option.Option<string>> {
  return await runOptional(async () => await readCanonicalDirectory(candidate));
}

async function resolveNodeDeps(projectRoot: string): Promise<ProjectDeps> {
  const candidate = join(projectRoot, 'node_modules');
  const nodeModules = await resolveDirectory(candidate);

  if (Option.isNone(nodeModules)) {
    return EMPTY_DEPS;
  }

  return { nodeModules: nodeModules.value, rustBuildCfgs: [], rustExterns: [] };
}

async function findSitePackagesInLib(libDir: string): Promise<Option.Option<string>> {
  const listed = await runOptional(async () => await readdir(libDir));

  if (Option.isNone(listed)) {
    return Option.none();
  }

  const pythonDirs = listed.value.filter((entry) => entry.startsWith('python')).toSorted();

  for (const entry of pythonDirs) {
    const candidate = join(libDir, entry, 'site-packages');
    // Check candidate Python version directories sequentially in deterministic order.
    // oxlint-disable-next-line no-await-in-loop
    const sitePackages = await resolveDirectory(candidate);

    if (Option.isSome(sitePackages)) {
      return sitePackages;
    }
  }

  return Option.none();
}

async function resolvePythonSitePackages(venvDir: string): Promise<Option.Option<string>> {
  const libCandidate = join(venvDir, 'lib');
  const libDir = await resolveDirectory(libCandidate);

  if (Option.isSome(libDir)) {
    return await findSitePackagesInLib(libDir.value);
  }

  const winCandidate = join(venvDir, 'Lib', 'site-packages');

  return await resolveDirectory(winCandidate);
}

async function resolvePythonDeps(projectRoot: string): Promise<ProjectDeps> {
  for (const name of ['.venv', 'venv']) {
    const venvDir = join(projectRoot, name);
    // Check standard virtualenv directory names sequentially in precedence order.
    // oxlint-disable-next-line no-await-in-loop
    const pythonSitePackages = await resolvePythonSitePackages(venvDir);

    if (Option.isSome(pythonSitePackages)) {
      return { pythonSitePackages: pythonSitePackages.value, rustBuildCfgs: [], rustExterns: [] };
    }
  }

  return EMPTY_DEPS;
}

async function resolveRustDeps(projectRoot: string): Promise<ProjectDeps> {
  const candidate = join(projectRoot, 'target', 'debug', 'deps');
  const rustDepsDir = await resolveDirectory(candidate);
  const build = await collectRustBuildArtifacts({ projectRoot });

  if (Option.isNone(rustDepsDir)) {
    return { ...build, rustExterns: [] };
  }

  const rustExterns = await collectRustExterns({ depsDir: rustDepsDir.value, projectRoot });

  return { ...build, rustDepsDir: rustDepsDir.value, rustExterns };
}

async function readProjectFile({
  name,
  projectRoot,
}: ProjectFileInput): Promise<Option.Option<string>> {
  const candidate = join(projectRoot, name);

  return await runOptional(async () => await readFile(candidate, 'utf-8'));
}

function readGoPathModCache(): string {
  const goPath = hostEnvironment.GOPATH?.split(delimiter)[0]?.trim() ?? '';

  if (goPath.length === 0) {
    return '';
  }

  return join(goPath, 'pkg', 'mod');
}

function readGoModCacheCandidates(): readonly string[] {
  const userHome = homedir();
  const configured = hostEnvironment.GOMODCACHE?.trim() ?? '';
  const goPathCache = readGoPathModCache();
  const defaultCache = join(userHome, 'go', 'pkg', 'mod');

  return [configured, goPathCache, defaultCache].filter((entry) => entry.length > 0);
}

async function resolveGoModCache(projectRoot: string): Promise<Option.Option<string>> {
  for (const candidate of readGoModCacheCandidates()) {
    // Check host Go module cache candidates sequentially in precedence order.
    // oxlint-disable-next-line no-await-in-loop
    const canonical = await resolveDirectory(candidate);

    if (Option.isSome(canonical)) {
      const prefix = `${canonical.value}${sep}`;

      if (canonical.value !== projectRoot && !projectRoot.startsWith(prefix)) {
        return canonical;
      }
    }
  }

  return Option.none();
}

async function resolveGoDeps(projectRoot: string): Promise<ProjectDeps> {
  const vendorCandidate = join(projectRoot, 'vendor');
  const vendorOption = await resolveDirectory(vendorCandidate);
  const modOption = await readProjectFile({ name: 'go.mod', projectRoot });
  const sumOption = await readProjectFile({ name: 'go.sum', projectRoot });
  const cacheOption = await resolveGoModCache(projectRoot);

  return {
    goMod: Option.getOrUndefined(modOption),
    goModCache: Option.getOrUndefined(cacheOption),
    goSum: Option.getOrUndefined(sumOption),
    goVendor: Option.getOrUndefined(vendorOption),
    rustBuildCfgs: [],
    rustExterns: [],
  };
}

async function selectDepsForRuntime({
  projectRoot,
  runtime,
}: SelectRuntimeDepsInput): Promise<ProjectDeps> {
  switch (runtime) {
    case 'go': {
      return await resolveGoDeps(projectRoot);
    }
    case 'node': {
      return await resolveNodeDeps(projectRoot);
    }
    case 'python': {
      return await resolvePythonDeps(projectRoot);
    }
    case 'rust': {
      return await resolveRustDeps(projectRoot);
    }
  }
}

function readSafeWithinRoot({ candidate, root }: WithinRootInput): Option.Option<string> {
  if (candidate === undefined) {
    return Option.none();
  }

  const prefix = `${root}${sep}`;

  if (!candidate.startsWith(prefix)) {
    return Option.none();
  }

  return Option.some(candidate);
}

function selectSafeRustExterns({ deps, rustDepsDir }: SelectSafeRustInput): readonly RustExtern[] {
  if (rustDepsDir === undefined) {
    return [];
  }

  return deps.rustExterns;
}

function filterRustDeps({ deps, root }: FilterRootInput): Pick<ProjectDeps, 'rustBuildCfgs' | 'rustBuildOutDir' | 'rustDepsDir' | 'rustExterns'> {
  const safeRust = readSafeWithinRoot({ candidate: deps.rustDepsDir, root });
  const rustDepsDir = Option.getOrUndefined(safeRust);
  const safeOut = readSafeWithinRoot({ candidate: deps.rustBuildOutDir, root });
  const rustBuildOutDir = Option.getOrUndefined(safeOut);
  const rustExterns = selectSafeRustExterns({ deps, rustDepsDir });

  return { rustBuildCfgs: deps.rustBuildCfgs, rustBuildOutDir, rustDepsDir, rustExterns };
}

function filterInsideRoot({ deps, root }: FilterRootInput): ProjectDeps {
  const safeGo = readSafeWithinRoot({ candidate: deps.goVendor, root });
  const safeNode = readSafeWithinRoot({ candidate: deps.nodeModules, root });
  const safePython = readSafeWithinRoot({ candidate: deps.pythonSitePackages, root });
  const rust = filterRustDeps({ deps, root });

  return {
    goMod: deps.goMod,
    goModCache: deps.goModCache,
    goSum: deps.goSum,
    goVendor: Option.getOrUndefined(safeGo),
    nodeModules: Option.getOrUndefined(safeNode),
    pythonSitePackages: Option.getOrUndefined(safePython),
    ...rust,
  };
}

/** Discover read-only project dependency directories already present on disk without running installs or builds. */
export async function resolveProjectDeps({
  projectRoot,
  runtime,
}: ResolveDepsInput): Promise<ProjectDeps> {
  if (projectRoot === undefined || projectRoot.length === 0) {
    return EMPTY_DEPS;
  }

  const canonicalRoot = await resolveDirectory(projectRoot);

  if (Option.isNone(canonicalRoot)) {
    return EMPTY_DEPS;
  }

  const deps = await selectDepsForRuntime({ projectRoot: canonicalRoot.value, runtime });

  return filterInsideRoot({ deps, root: canonicalRoot.value });
}
