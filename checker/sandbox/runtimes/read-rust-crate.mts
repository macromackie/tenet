import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { Option } from 'effect';
import { runOptional } from '../run-optional.mts';
import { parseCargoManifest } from './parse-cargo-manifest.mts';

const MAX_FEATURES = 32;

export type RustCrate = Readonly<{
  edition: string;
  features: readonly string[];
  libPath: string;
  name: string;
}>;

type ExpandFeaturesInput = Readonly<{
  featureEntries: ReadonlyMap<string, readonly string[]>;
  seeds: readonly string[];
}>;

type QueueState = Readonly<{
  enabled: readonly string[];
  queue: readonly string[];
  seenFeatures: ReadonlySet<string>;
}>;

type AdvanceQueueInput = Readonly<{
  featureEntries: ReadonlyMap<string, readonly string[]>;
  state: QueueState;
}>;

type ReadRustCrateInput = Readonly<{
  files: ReadonlyMap<string, string>;
  projectRoot?: string;
}>;

function isPlainFeatureName(value: string): boolean {
  return value.length > 0 && !value.includes('/') && !value.startsWith('dep:');
}

function advanceFeatureQueue({ featureEntries, state }: AdvanceQueueInput): QueueState {
  const current = state.queue.at(0);
  const rest = state.queue.slice(1);

  if (current === undefined || state.seenFeatures.has(current) || !isPlainFeatureName(current)) {
    return { ...state, queue: rest };
  }

  const nextSeenFeatures = new Set<string>([...state.seenFeatures, current]);
  const children = featureEntries.get(current) ?? [];

  return {
    enabled: [...state.enabled, current],
    queue: [...rest, ...children],
    seenFeatures: nextSeenFeatures,
  };
}

function listEnabledFeatures({ featureEntries, seeds }: ExpandFeaturesInput): readonly string[] {
  let state: QueueState = {
    enabled: [],
    queue: [...seeds],
    seenFeatures: new Set<string>(),
  };

  while (state.queue.length > 0 && state.enabled.length < MAX_FEATURES) {
    state = advanceFeatureQueue({ featureEntries, state });
  }

  return state.enabled;
}

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

function buildRustCrate(manifestText: string): Option.Option<RustCrate> {
  const parsed = parseCargoManifest(manifestText);
  const resolvedName = Option.orElse(parsed.libName, () => parsed.packageName);

  if (Option.isNone(resolvedName)) {
    return Option.none();
  }

  const defaultSeeds = parsed.featureEntries.get('default') ?? [];

  const features = listEnabledFeatures({
    featureEntries: parsed.featureEntries,
    seeds: defaultSeeds,
  });

  return Option.some({
    edition: parsed.edition,
    features,
    libPath: 'src/lib.rs',
    name: resolvedName.value,
  });
}

/** Extract crate name, edition, and default features when a staged Rust project has `src/lib.rs`. */
export async function readRustCrate({
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

  return buildRustCrate(manifestText.value);
}
