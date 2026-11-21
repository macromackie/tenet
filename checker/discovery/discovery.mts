import { basename, dirname, extname } from 'node:path';
import { Option, Result } from 'effect';
import { classifyResult } from '../classify-result.mts';
import { hashSource, readRegular } from '../storage/index.mts';
import type { Contract, Inventory } from '../types.d.mts';
import { hashSurface } from './hash-surface.mts';
import { MANIFESTS } from './manifests.mts';
import { listRankedPaths } from './rank-path.mts';

const EXECUTION = { bytes: 16_777_216, files: 1_024, source: 524_288 } as const;
const MAX_FILES = 160;
const MAX_BYTES = 307_200;
const MAX_SOURCE = 131_072;
const MAX_SAMPLES = 32_768;
const MAX_SAMPLE = 8_192;

const EXTENSIONS: ReadonlySet<string> = new Set([
  '.js',
  '.cjs',
  '.mjs',
  '.ts',
  '.mts',
  '.cts',
  '.tsx',
  '.jsx',
  '.py',
  '.rs',
  '.go',
]);

const UNSUPPORTED_EXTENSIONS: ReadonlySet<string> = new Set([
  '.java',
  '.c',
  '.cc',
  '.cpp',
  '.swift',
  '.rb',
  '.cs',
  '.kt',
]);

const PACKAGE_DATA_EXTENSIONS: ReadonlySet<string> = new Set(['.txt', '.json', '.csv']);

type Selected = { files: Map<string, string>; omitted: readonly string[] };

type ClassifyPathInput = {
  readonly path: string;
  readonly decision?: string;
  readonly packageRoots: readonly string[];
};

function classifyPath({
  path,
  decision,
  packageRoots,
}: ClassifyPathInput): 'read' | 'omitted' | 'skip' {
  const extension = extname(path);
  const name = basename(path);

  if (EXTENSIONS.has(extension) || MANIFESTS.has(name)) {
    return 'read';
  }

  if (
    PACKAGE_DATA_EXTENSIONS.has(extension) &&
    packageRoots.some((root) => path.startsWith(`${root}/`))
  ) {
    return 'read';
  }

  if (
    (UNSUPPORTED_EXTENSIONS.has(extension) && decision !== 'not_applicable') ||
    decision === 'fail'
  ) {
    return 'omitted';
  }

  return 'skip';
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

  const packageModules = input.contract.paths.filter((path) => path.endsWith('/__init__.py'));

  return packageModules.map((path) => dirname(path));
}

async function select(input: SelectInput): Promise<Selected> {
  const packageRoots = listPackageRoots(input);
  const pool = createCapture(input);

  for (const path of listRankedPaths(input.contract)) {
    const decision = input.contract.decisions?.[path];
    const kind = classifyPath({ decision, packageRoots, path });

    if (kind === 'omitted') {
      pool.markOmitted(path);
    }

    if (kind === 'read') {
      // The preceding read determines the remaining shared byte allowance.
      // oxlint-disable-next-line no-await-in-loop
      await pool.capture(path);
    }
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

/** Keep the provider packet and local execution closure independently bounded. */
export async function readInventory(input: {
  root: string;
  contract: Contract;
}): Promise<Inventory> {
  const { contract, root } = input;
  const { files, omitted: unprompted } = await select({ ...input, budget: PROMPT });
  const dependencies = await select({ ...input, budget: EXECUTION });
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
