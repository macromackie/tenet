const MAX_FEATURES = 32;
const DEP_PREFIX = 'dep:';

const SKIPPED_FEATURE_NAMES: ReadonlySet<string> = new Set([
  'bench',
  'benches',
  'ci',
  'default',
  'docs',
  'internal',
  'test',
  'tests',
]);

const SKIPPED_FEATURE_PREFIXES = ['__', 'no-', 'no_', 'unstable'] as const;

type SelectFeaturesInput = Readonly<{
  externNames: ReadonlySet<string>;
  featureEntries: ReadonlyMap<string, readonly string[]>;
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

type CheckFeatureInput = Readonly<{
  activeFeatures: ReadonlySet<string>;
  externNames: ReadonlySet<string>;
  featureEntries: ReadonlyMap<string, readonly string[]>;
  name: string;
}>;

function isPlainFeatureName(value: string): boolean {
  return value.length > 0 && !value.includes('/') && !value.startsWith('dep:');
}

function isSkippedFeature(name: string): boolean {
  if (SKIPPED_FEATURE_NAMES.has(name)) {
    return true;
  }

  return SKIPPED_FEATURE_PREFIXES.some((prefix) => name.startsWith(prefix));
}

function normalizeCrateName(raw: string): string {
  return raw.replaceAll('-', '_');
}

function readDepCrateName(item: string): string {
  if (item.startsWith(DEP_PREFIX)) {
    const depPart = item.slice(DEP_PREFIX.length);

    return normalizeCrateName(depPart);
  }

  const slashIndex = item.indexOf('/');
  const head = item.slice(0, slashIndex).replace(/\?$/v, '');

  return normalizeCrateName(head);
}

type ExternalItemInput = Readonly<{
  externNames: ReadonlySet<string>;
  item: string;
}>;

function isExternalItemSatisfied({ externNames, item }: ExternalItemInput): boolean {
  if (!isPlainFeatureName(item)) {
    const depName = readDepCrateName(item);

    return externNames.has(depName);
  }

  const crateName = normalizeCrateName(item);

  return externNames.has(crateName);
}

function isFeatureSatisfied({
  activeFeatures,
  externNames,
  featureEntries,
  name,
}: CheckFeatureInput): boolean {
  if (activeFeatures.has(name)) {
    return true;
  }

  const items = featureEntries.get(name);

  if (items === undefined) {
    return false;
  }

  const nextActiveFeatures = new Set<string>([...activeFeatures, name]);

  return items.every((item) => {
    if (isPlainFeatureName(item) && featureEntries.has(item)) {
      return isFeatureSatisfied({
        activeFeatures: nextActiveFeatures,
        externNames,
        featureEntries,
        name: item,
      });
    }

    return isExternalItemSatisfied({ externNames, item });
  });
}

function listOptionalSeeds({
  externNames,
  featureEntries,
}: SelectFeaturesInput): readonly string[] {
  const seeds: string[] = [];
  const initialFeatures = new Set<string>();

  for (const name of [...featureEntries.keys()].toSorted()) {
    if (
      !isSkippedFeature(name) &&
      isFeatureSatisfied({ activeFeatures: initialFeatures, externNames, featureEntries, name })
    ) {
      seeds.push(name);
    }
  }

  return seeds;
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

/** Expand Cargo default features plus local optional features whose dependencies are already built. */
export function selectRustFeatures({
  externNames,
  featureEntries,
}: SelectFeaturesInput): readonly string[] {
  const defaultSeeds = featureEntries.get('default') ?? [];
  const optionalSeeds = listOptionalSeeds({ externNames, featureEntries });

  let state: QueueState = {
    enabled: [],
    queue: [...defaultSeeds, ...optionalSeeds],
    seenFeatures: new Set<string>(),
  };

  while (state.queue.length > 0 && state.enabled.length < MAX_FEATURES) {
    state = advanceFeatureQueue({ featureEntries, state });
  }

  return state.enabled;
}
