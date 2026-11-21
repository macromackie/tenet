import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { Option } from 'effect';
import type { ProjectDeps } from '../project-deps.mts';
import { runOptional } from '../run-optional.mts';
import { parseCargoManifest } from './parse-cargo-manifest.mts';
import { selectRustFeatures } from './select-rust-features.mts';

export type RustCrate = Readonly<{
  edition: string;
  features: readonly string[];
  libPath: string;
  name: string;
}>;

type ReadRustCrateInput = Readonly<{
  deps?: ProjectDeps;
  files: ReadonlyMap<string, string>;
  projectRoot?: string;
}>;

type BuildRustCrateInput = Readonly<{
  externNames: ReadonlySet<string>;
  manifestText: string;
}>;

async function readManifestText({
  files,
  projectRoot,
}: ReadRustCrateInput): Promise<Option.Option<string>> {
  const staged = files.get('Cargo.toml');

  if (staged !== undefined) {
    return Option.some(staged);
  }

  if (projectRoot === undefined || projectRoot.length === 0) {
    return Option.none();
  }

  const manifestPath = join(projectRoot, 'Cargo.toml');

  return await runOptional(async () => await readFile(manifestPath, 'utf-8'));
}

function readExternNames(deps?: ProjectDeps): ReadonlySet<string> {
  if (deps === undefined) {
    return new Set<string>();
  }

  const names = deps.rustExterns.map((entry) => entry.name);

  return new Set<string>(names);
}

function buildRustCrate({ externNames, manifestText }: BuildRustCrateInput): Option.Option<RustCrate> {
  const parsed = parseCargoManifest(manifestText);
  const resolvedName = Option.orElse(parsed.libName, () => parsed.packageName);

  if (Option.isNone(resolvedName)) {
    return Option.none();
  }

  const features = selectRustFeatures({
    externNames,
    featureEntries: parsed.featureEntries,
  });

  return Option.some({
    edition: parsed.edition,
    features,
    libPath: 'src/lib.rs',
    name: resolvedName.value,
  });
}

/** Extract crate name, edition, and satisfiable features when a staged Rust project has `src/lib.rs`. */
export async function readRustCrate({
  deps,
  files,
  projectRoot,
}: ReadRustCrateInput): Promise<Option.Option<RustCrate>> {
  if (!files.has('src/lib.rs')) {
    return Option.none();
  }

  const manifestText = await readManifestText({ files, projectRoot });

  if (Option.isNone(manifestText)) {
    return Option.none();
  }

  const externNames = readExternNames(deps);

  return buildRustCrate({ externNames, manifestText: manifestText.value });
}
