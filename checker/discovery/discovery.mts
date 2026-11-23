import { basename, dirname, extname } from 'node:path';
import { Option, Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import { hashSource, readRegular } from '../storage/index.mts';
import type { Contract, Inventory } from '../types.d.mts';
import { hashSurface } from './hash-surface.mts';
import { isHiddenDirectoryPath } from './is-hidden-dir.mts';
import { isTestOrFixturePath } from './is-test-path.mts';
import {
  EXTENSIONS,
  MANIFESTS,
  PACKAGE_DATA_EXTENSIONS,
  UNSUPPORTED_EXTENSIONS,
} from './manifests.mts';
import { listRankedPaths } from './rank-path.mts';

const EXECUTION = { bytes: 16_777_216, files: 1_024, source: 524_288 } as const;
const MAX_FILES = 240;
const MAX_BYTES = 524_288;
const MAX_SOURCE = 131_072;
const MAX_SAMPLES = 32_768;
const MAX_SAMPLE = 8_192;

type Selected = { files: Map<string, string>; omitted: readonly string[] };

type ClassifyPathInput = {
  readonly path: string;
  readonly decision?: string;
  readonly excludeTests: boolean;
  readonly packageRoots: readonly string[];
};

type ReadablePathInput = Readonly<{
  decision?: string;
  excludeTests: boolean;
  path: string;
}>;

type PackageDataInput = Readonly<{
  packageRoots: readonly string[];
  path: string;
}>;

function classifyReadablePath({ decision, excludeTests, path }: ReadablePathInput): 'read' | 'skip' {
  if (excludeTests && decision !== 'fail' && isTestOrFixturePath(path)) {
    return 'skip';
  }

  return 'read';
}

function isPackageDataPath({ packageRoots, path }: PackageDataInput): boolean {
  const extension = extname(path);

  return (
    PACKAGE_DATA_EXTENSIONS.has(extension) &&
    packageRoots.some((root) => path.startsWith(`${root}/`))
  );
}

type NonCodeInput = Readonly<{
  decision?: string;
  extension: string;
  packageRoots: readonly string[];
  path: string;
}>;

function classifyNonCodePath({
  decision,
  extension,
  packageRoots,
  path,
}: NonCodeInput): 'read' | 'omitted' | 'skip' {
  if (isPackageDataPath({ packageRoots, path })) {
    return 'read';
  }

  if (decision === 'fail' || (UNSUPPORTED_EXTENSIONS.has(extension) && decision !== 'not_applicable')) {
    return 'omitted';
  }

  return 'skip';
}

function classifyPath({
  path,
  decision,
  excludeTests,
  packageRoots,
}: ClassifyPathInput): 'read' | 'omitted' | 'skip' {
  if (isHiddenDirectoryPath(path) && decision !== 'fail') {
    return 'skip';
  }

  const extension = extname(path);
  const name = basename(path);

  if (EXTENSIONS.has(extension) || MANIFESTS.has(name)) {
    return classifyReadablePath({ decision, excludeTests, path });
  }

  return classifyNonCodePath({ decision, extension, packageRoots, path });
}

type ReadSampleInput = { readonly root: string; readonly path: string; readonly limit: number };

async function readSample(input: ReadSampleInput): Promise<Option.Option<string>> {
  async function runRead() {
    return await readRegular(input);
  }

  const result = await classifyResult({ message: 'bounded source unavailable', run: runRead });

  return Result.getSuccess(result);
}

type Budget = { bytes: number; files: number; source: number };

type SelectInput = {
  budget: Budget;
  contract: Contract;
  root: string;
};

type CreateCaptureInput = { readonly budget: Budget; readonly root: string };

function createCapture({ root, budget }: CreateCaptureInput) {
  const files = new Map<string, string>();
  const omitted: string[] = [];
  let remaining = budget.bytes;

  async function capture(path: string) {
    if (files.size >= budget.files || remaining <= 0) {
      omitted.push(path);

      return;
    }

    const limit = Math.min(remaining, budget.source);
    const result = await readSample({ limit, path, root });

    if (Option.isNone(result)) {
      omitted.push(path);

      return;
    }

    files.set(path, result.value);

    remaining -= Buffer.byteLength(result.value);
  }

  function markOmitted(path: string) {
    omitted.push(path);
  }

  return { capture, files, markOmitted, omitted };
}

type ReadSamplesInput = {
  paths: readonly string[];
  root: string;
};

async function readSamples({ root, paths }: ReadSamplesInput): Promise<Record<string, string>> {
  const budget = { bytes: MAX_SAMPLES, files: paths.length, source: MAX_SAMPLE };
  const pool = createCapture({ budget, root });

  for (const path of paths.toSorted()) {
    // Bound concurrent reads to the remaining packet size.
    // oxlint-disable-next-line no-await-in-loop
    await pool.capture(path);
  }

  // ast-grep-ignore: object-from-entries-typescript -- The fx generation packet serializes contract samples as a path-keyed JSON object.
  return Object.fromEntries(pool.files);
}

function listPackageRoots(input: SelectInput): readonly string[] {
  if (input.budget !== EXECUTION) {
    return [];
  }

  const packageModules = input.contract.paths.filter(
    (path) => path.endsWith('/__init__.py') && !isHiddenDirectoryPath(path),
  );

  return packageModules.map((path) => dirname(path));
}

function hasProductionCode(paths: readonly string[]): boolean {
  return paths.some((path) => {
    const extension = extname(path);

    return EXTENSIONS.has(extension) && !isHiddenDirectoryPath(path) && !isTestOrFixturePath(path);
  });
}

type ApplyPathInput = Readonly<{
  kind: 'read' | 'omitted' | 'skip';
  path: string;
  pool: ReturnType<typeof createCapture>;
}>;

async function applyPath({ kind, path, pool }: ApplyPathInput): Promise<void> {
  if (kind === 'omitted') {
    pool.markOmitted(path);

    return;
  }

  if (kind === 'read') {
    await pool.capture(path);
  }
}

async function select(input: SelectInput): Promise<Selected> {
  const packageRoots = listPackageRoots(input);
  const rankedPaths = listRankedPaths(input.contract);
  const excludeTests = input.budget !== EXECUTION && hasProductionCode(rankedPaths);
  const pool = createCapture(input);

  for (const path of rankedPaths) {
    const decision = input.contract.decisions?.[path];
    const kind = classifyPath({ decision, excludeTests, packageRoots, path });

    // The preceding read determines the remaining shared byte allowance.
    // oxlint-disable-next-line no-await-in-loop
    await applyPath({ kind, path, pool });
  }

  return pool;
}

function listEntries(files: ReadonlyMap<string, string>): readonly [string, string][] {
  const result: [string, string][] = [];

  for (const path of [...files.keys()].toSorted()) {
    const value = files.get(path);

    if (value !== undefined) {
      result.push([path, value]);
    }
  }

  return result;
}

function hashContents(files: ReadonlyMap<string, string>) {
  const contents = listEntries(files);
  const serialized = JSON.stringify(contents);

  return hashSource(serialized);
}

const PROMPT = { bytes: MAX_BYTES, files: MAX_FILES, source: MAX_SOURCE } as const;

function buildExecutionContract(contract: Contract): Contract {
  if (contract.runtimePaths === undefined || contract.runtimePaths.length === 0) {
    return contract;
  }

  const combinedPaths = new Set<string>([...contract.paths, ...contract.runtimePaths]);

  return { ...contract, paths: [...combinedPaths] };
}

async function selectExecution(input: { contract: Contract; root: string }): Promise<Selected> {
  const executionContract = buildExecutionContract(input.contract);

  return await select({ budget: EXECUTION, contract: executionContract, root: input.root });
}

/** Keep the provider packet and local execution closure independently bounded. */
export async function readInventory(input: {
  root: string;
  contract: Contract;
}): Promise<Inventory> {
  const { contract, root } = input;
  const { files, omitted: unprompted } = await select({ ...input, budget: PROMPT });
  const dependencies = await selectExecution(input);
  const omittedPaths = new Set([...unprompted, ...dependencies.omitted]);
  const omitted = [...omittedPaths];
  const assets = await readSamples({ paths: contract.assets ?? [], root });
  const source = listEntries(files);
  const shape = hashSurface({ assets, contract, omitted, source });
  const observed = hashContents(dependencies.files);

  return {
    assets,
    decisions: contract.decisions ?? {},
    files,
    observed,
    omitted,
    runtimeFiles: dependencies.files,
    shape,
  };
}
