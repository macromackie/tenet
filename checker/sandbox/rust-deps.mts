import { readdir, readFile, stat } from 'node:fs/promises';
import { join } from 'node:path';
import { Option } from 'effect';
import { runOptional } from './run-optional.mts';

const MAX_RUST_EXTERNS = 64;
const RUST_CRATE_PATTERN = /^lib(?<name>[A-Za-z0-9_]+)-[0-9a-f]{16}\.(?<ext>rlib|dylib|so)$/v;
const CARGO_NAME_PATTERN = /^\s*name\s*=\s*"(?<name>[^"]+)"/mv;

export type RustExtern = Readonly<{
  file: string;
  name: string;
}>;

type RustArtifact = Readonly<{
  file: string;
  mtimeMs: number;
  name: string;
}>;

type RustArtifactInput = Readonly<{
  depsDir: string;
  entry: string;
  selfCrate: Option.Option<string>;
}>;

type CollectRustDepsInput = Readonly<{
  depsDir: string;
  projectRoot: string;
}>;

async function readCargoPackageName(projectRoot: string): Promise<Option.Option<string>> {
  const manifestPath = join(projectRoot, 'Cargo.toml');
  const manifest = await runOptional(async () => await readFile(manifestPath, 'utf-8'));

  if (Option.isNone(manifest)) {
    return Option.none();
  }

  const match = CARGO_NAME_PATTERN.exec(manifest.value);
  const rawName = match?.groups?.['name'];

  if (rawName === undefined) {
    return Option.none();
  }

  const normalized = rawName.replaceAll('-', '_');

  return Option.some(normalized);
}

async function readRustFileMtime(fullPath: string): Promise<Option.Option<number>> {
  const info = await runOptional(async () => await stat(fullPath));

  if (Option.isNone(info) || !info.value.isFile()) {
    return Option.none();
  }

  return Option.some(info.value.mtimeMs);
}

async function readRustArtifact({
  depsDir,
  entry,
  selfCrate,
}: RustArtifactInput): Promise<Option.Option<RustArtifact>> {
  const match = RUST_CRATE_PATTERN.exec(entry);
  const name = match?.groups?.['name'];
  const selfName = Option.getOrUndefined(selfCrate);

  if (name === undefined || name === selfName) {
    return Option.none();
  }

  const artifactPath = join(depsDir, entry);
  const mtimeMs = await readRustFileMtime(artifactPath);

  if (Option.isNone(mtimeMs)) {
    return Option.none();
  }

  return Option.some({ file: entry, mtimeMs: mtimeMs.value, name });
}

function listOrderedExterns(
  latestArtifacts: ReadonlyMap<string, RustArtifact>,
): readonly RustExtern[] {
  const orderedNames = [...latestArtifacts.keys()].toSorted().slice(0, MAX_RUST_EXTERNS);
  const externs: RustExtern[] = [];

  for (const name of orderedNames) {
    const artifact = latestArtifacts.get(name);

    if (artifact !== undefined) {
      externs.push({ file: artifact.file, name: artifact.name });
    }
  }

  return externs;
}

function indexLatestArtifacts(
  artifacts: readonly RustArtifact[],
): ReadonlyMap<string, RustArtifact> {
  const latestArtifacts = new Map<string, RustArtifact>();

  for (const artifact of artifacts) {
    const prior = latestArtifacts.get(artifact.name);

    if (prior === undefined || artifact.mtimeMs > prior.mtimeMs) {
      latestArtifacts.set(artifact.name, artifact);
    }
  }

  return latestArtifacts;
}

type ReadAllArtifactsInput = Readonly<{
  depsDir: string;
  entries: readonly string[];
  selfCrate: Option.Option<string>;
}>;

async function readAllArtifacts({
  depsDir,
  entries,
  selfCrate,
}: ReadAllArtifactsInput): Promise<readonly RustArtifact[]> {
  const found: RustArtifact[] = [];

  for (const entry of entries) {
    // Stat artifacts sequentially to keep deterministic selection across filesystems.
    // oxlint-disable-next-line no-await-in-loop
    const artifact = await readRustArtifact({ depsDir, entry, selfCrate });

    if (Option.isSome(artifact)) {
      found.push(artifact.value);
    }
  }

  return found;
}

/** Collect prebuilt external crate artifacts from an existing Cargo target/debug/deps directory. */
export async function collectRustExterns({
  depsDir,
  projectRoot,
}: CollectRustDepsInput): Promise<readonly RustExtern[]> {
  const listed = await runOptional(async () => await readdir(depsDir));

  if (Option.isNone(listed)) {
    return [];
  }

  const selfCrate = await readCargoPackageName(projectRoot);
  const entries = listed.value.toSorted();
  const found = await readAllArtifacts({ depsDir, entries, selfCrate });
  const latestArtifacts = indexLatestArtifacts(found);

  return listOrderedExterns(latestArtifacts);
}
