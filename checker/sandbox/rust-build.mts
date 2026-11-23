import { readdir, readFile, realpath, stat } from 'node:fs/promises';
import { join, relative, sep } from 'node:path';
import { Option } from 'effect';
import { readRegular } from '../storage/index.mts';
import { runOptional } from './run-optional.mts';

const MAX_BUILD_CFGS = 64;
const MAX_OUTPUT_BYTES = 262_144;
const BUILD_DIR_PATTERN = /^(?<pkg>.+)-[0-9a-f]{16}$/v;
const RUSTC_CFG_LINE = /^cargo::?rustc-cfg=(?<cfg>.+)$/v;
const SAFE_CFG_PATTERN = /^[A-Za-z0-9_]+(?:="[^"\n\r\\]*")?$/v;
const SECTION_HEADER_PATTERN = /^\s*\[(?<name>[^\]]+)\]\s*$/v;
const PACKAGE_NAME_PATTERN = /^\s*name\s*=\s*"(?<name>[^"]+)"/v;

type RustBuildArtifacts = Readonly<{
  rustBuildCfgs: readonly string[];
  rustBuildOutDir?: string;
}>;

type CollectRustBuildInput = Readonly<{ projectRoot: string }>;

type BuildCandidate = Readonly<{
  dirPath: string;
  hasOutDir: boolean;
  hasOutputFile: boolean;
  mtimeMs: number;
}>;

type CandidateCheckInput = Readonly<{
  buildRoot: string;
  entry: string;
  packageNames: ReadonlySet<string>;
}>;

type SelectNewestInput = Readonly<{ canonicalRoot: string; packageNames: ReadonlySet<string> }>;
type DirReadInput = Readonly<{ canonicalRoot: string; dirPath: string }>;
type ReadArtifactsInput = Readonly<{ candidate: BuildCandidate; canonicalRoot: string }>;

const EMPTY_BUILD_ARTIFACTS = { rustBuildCfgs: [] } as const satisfies RustBuildArtifacts;

function readPackageNameLine(line: string): Option.Option<string> {
  const match = PACKAGE_NAME_PATTERN.exec(line);
  const name = match?.groups?.['name'];

  if (name === undefined || name.length === 0) {
    return Option.none();
  }

  return Option.some(name);
}

function readSectionName(line: string): Option.Option<string> {
  const header = SECTION_HEADER_PATTERN.exec(line);
  const name = header?.groups?.['name']?.trim();

  return Option.fromNullishOr(name);
}

type PackageScanState = Readonly<{ inPackage: boolean; name: Option.Option<string> }>;
type PackageScanStepInput = Readonly<{ rawLine: string; state: PackageScanState }>;

function advancePackageScan({ rawLine, state }: PackageScanStepInput): PackageScanState {
  if (Option.isSome(state.name)) {
    return state;
  }

  const line = rawLine.split('#')[0] ?? '';
  const section = readSectionName(line);

  if (Option.isSome(section)) {
    return { inPackage: section.value === 'package', name: Option.none() };
  }

  if (!state.inPackage) {
    return state;
  }

  return { inPackage: true, name: readPackageNameLine(line) };
}

function extractRawPackageName(manifest: string): Option.Option<string> {
  let state: PackageScanState = { inPackage: false, name: Option.none() };

  for (const rawLine of manifest.split('\n')) {
    state = advancePackageScan({ rawLine, state });
  }

  return state.name;
}

function buildPackageNameVariants(rawName: string): ReadonlySet<string> {
  const underscored = rawName.replaceAll('-', '_');
  const hyphenated = rawName.replaceAll('_', '-');

  return new Set([rawName, underscored, hyphenated]);
}

async function readPackageNameVariants(projectRoot: string): Promise<ReadonlySet<string>> {
  const manifestPath = join(projectRoot, 'Cargo.toml');
  const manifest = await runOptional(async () => await readFile(manifestPath, 'utf-8'));

  if (Option.isNone(manifest)) {
    return new Set();
  }

  const rawName = extractRawPackageName(manifest.value);

  if (Option.isNone(rawName)) {
    return new Set();
  }

  return buildPackageNameVariants(rawName.value);
}

async function hasRegularFile(path: string): Promise<boolean> {
  const info = await runOptional(async () => await stat(path));

  return Option.isSome(info) && info.value.isFile();
}

async function hasDirectory(path: string): Promise<boolean> {
  const info = await runOptional(async () => await stat(path));

  return Option.isSome(info) && info.value.isDirectory();
}

async function readCandidateDirectory(dirPath: string): Promise<Option.Option<BuildCandidate>> {
  const dirStat = await runOptional(async () => await stat(dirPath));

  if (Option.isNone(dirStat) || !dirStat.value.isDirectory()) {
    return Option.none();
  }

  const outputPath = join(dirPath, 'output');
  const outPath = join(dirPath, 'out');
  const hasOutputFile = await hasRegularFile(outputPath);
  const hasOutDir = await hasDirectory(outPath);

  if (!hasOutputFile && !hasOutDir) {
    return Option.none();
  }

  return Option.some({ dirPath, hasOutDir, hasOutputFile, mtimeMs: dirStat.value.mtimeMs });
}

async function inspectBuildCandidate({
  buildRoot,
  entry,
  packageNames,
}: CandidateCheckInput): Promise<Option.Option<BuildCandidate>> {
  const match = BUILD_DIR_PATTERN.exec(entry);
  const pkg = match?.groups?.['pkg'];

  if (pkg === undefined || !packageNames.has(pkg)) {
    return Option.none();
  }

  const dirPath = join(buildRoot, entry);

  return await readCandidateDirectory(dirPath);
}

async function selectNewestBuildDir({
  canonicalRoot,
  packageNames,
}: SelectNewestInput): Promise<Option.Option<BuildCandidate>> {
  const buildRoot = join(canonicalRoot, 'target', 'debug', 'build');
  const listed = await runOptional(async () => await readdir(buildRoot));

  if (Option.isNone(listed)) {
    return Option.none();
  }

  let newest = Option.none<BuildCandidate>();

  for (const entry of listed.value.toSorted()) {
    // Stat candidate Cargo build script directories sequentially for deterministic tie-breaking.
    // oxlint-disable-next-line no-await-in-loop
    const candidate = await inspectBuildCandidate({ buildRoot, entry, packageNames });

    if (
      Option.isSome(candidate) &&
      (Option.isNone(newest) || candidate.value.mtimeMs > newest.value.mtimeMs)
    ) {
      newest = candidate;
    }
  }

  return newest;
}

function parseSafeRustCfgs(outputText: string): readonly string[] {
  const foundCfgs = new Set<string>();

  for (const rawLine of outputText.split('\n')) {
    const trimmed = rawLine.trim();
    const match = RUSTC_CFG_LINE.exec(trimmed);
    const cfg = match?.groups?.['cfg']?.trim();

    if (cfg !== undefined && SAFE_CFG_PATTERN.test(cfg)) {
      foundCfgs.add(cfg);
    }
  }

  return [...foundCfgs].toSorted().slice(0, MAX_BUILD_CFGS);
}

async function readBuildCfgs({ canonicalRoot, dirPath }: DirReadInput): Promise<readonly string[]> {
  const outputPath = join(dirPath, 'output');
  const relativePath = relative(canonicalRoot, outputPath);

  const text = await runOptional(
    async () => await readRegular({ limit: MAX_OUTPUT_BYTES, path: relativePath, root: canonicalRoot, truncate: true }),
  );

  if (Option.isNone(text)) {
    return [];
  }

  return parseSafeRustCfgs(text.value);
}

async function resolveSafeOutDir({
  canonicalRoot,
  dirPath,
}: DirReadInput): Promise<Option.Option<string>> {
  const outPath = join(dirPath, 'out');
  const canonicalOut = await runOptional(async () => await realpath(outPath));

  if (Option.isNone(canonicalOut)) {
    return Option.none();
  }

  const prefix = `${canonicalRoot}${sep}`;

  if (!canonicalOut.value.startsWith(prefix) || !(await hasDirectory(canonicalOut.value))) {
    return Option.none();
  }

  return canonicalOut;
}

async function readOptionalCfgs({
  candidate,
  canonicalRoot,
}: ReadArtifactsInput): Promise<readonly string[]> {
  if (!candidate.hasOutputFile) {
    return [];
  }

  return await readBuildCfgs({ canonicalRoot, dirPath: candidate.dirPath });
}

async function readOptionalOutDir({
  candidate,
  canonicalRoot,
}: ReadArtifactsInput): Promise<Option.Option<string>> {
  if (!candidate.hasOutDir) {
    return Option.none();
  }

  return await resolveSafeOutDir({ canonicalRoot, dirPath: candidate.dirPath });
}

async function readBuildDirArtifacts(input: ReadArtifactsInput): Promise<RustBuildArtifacts> {
  const rustBuildCfgs = await readOptionalCfgs(input);
  const outDir = await readOptionalOutDir(input);

  return { rustBuildCfgs, rustBuildOutDir: Option.getOrUndefined(outDir) };
}

/** Collect prebuilt Cargo `build.rs` `rustc-cfg` flags and read-only `OUT_DIR` without executing build scripts. */
export async function collectRustBuildArtifacts({
  projectRoot,
}: CollectRustBuildInput): Promise<RustBuildArtifacts> {
  const canonicalRoot = await runOptional(async () => await realpath(projectRoot));

  if (Option.isNone(canonicalRoot)) {
    return EMPTY_BUILD_ARTIFACTS;
  }

  const packageNames = await readPackageNameVariants(canonicalRoot.value);

  if (packageNames.size === 0) {
    return EMPTY_BUILD_ARTIFACTS;
  }

  const newest = await selectNewestBuildDir({ canonicalRoot: canonicalRoot.value, packageNames });

  if (Option.isNone(newest)) {
    return EMPTY_BUILD_ARTIFACTS;
  }

  return await readBuildDirArtifacts({ candidate: newest.value, canonicalRoot: canonicalRoot.value });
}
