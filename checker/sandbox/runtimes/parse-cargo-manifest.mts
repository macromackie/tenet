import { Option } from 'effect';

const SECTION_PATTERN = /^\s*\[(?<section>[^\]]+)\]\s*$/v;
const STRING_FIELD_PATTERN = /^\s*(?<key>[A-Za-z0-9_\-]+)\s*=\s*"(?<value>[^"]+)"/v;
const LIST_FIELD_PATTERN = /^\s*(?<key>[A-Za-z0-9_\-]+)\s*=\s*\[(?<items>[^\]]*)\]/v;
const QUOTED_ITEM_PATTERN = /"(?<item>[^"]+)"/gv;
const VALID_EDITIONS: ReadonlySet<string> = new Set(['2015', '2018', '2021', '2024']);

type CargoManifest = Readonly<{
  edition: string;
  featureEntries: ReadonlyMap<string, readonly string[]>;
  libName: Option.Option<string>;
  packageName: Option.Option<string>;
}>;

type ManifestState = CargoManifest & Readonly<{ section: string }>;

type ManifestLineInput = Readonly<{ line: string; state: ManifestState }>;

function readQuotedItems(itemsText: string): readonly string[] {
  const values: string[] = [];

  for (const match of itemsText.matchAll(QUOTED_ITEM_PATTERN)) {
    const item = match.groups?.['item'];

    if (item !== undefined && item.length > 0) {
      values.push(item);
    }
  }

  return values;
}

function readSection(line: string): Option.Option<string> {
  const match = SECTION_PATTERN.exec(line);
  const name = match?.groups?.['section'];

  if (name === undefined) {
    return Option.none();
  }

  const trimmed = name.trim();

  return Option.some(trimmed);
}

function normalizeEdition(raw: string): string {
  if (VALID_EDITIONS.has(raw)) {
    return raw;
  }

  return '2021';
}

function readStringField(line: string): Option.Option<Readonly<{ key: string; value: string }>> {
  const pair = STRING_FIELD_PATTERN.exec(line);
  const key = pair?.groups?.['key'];
  const value = pair?.groups?.['value'];

  if (key === undefined || value === undefined) {
    return Option.none();
  }

  return Option.some({ key, value });
}

function updatePackageState({ line, state }: ManifestLineInput): ManifestState {
  const field = readStringField(line);

  if (Option.isNone(field)) {
    return state;
  }

  if (field.value.key === 'name') {
    const normalized = field.value.value.replaceAll('-', '_');

    return { ...state, packageName: Option.some(normalized) };
  }

  if (field.value.key === 'edition') {
    const edition = normalizeEdition(field.value.value);

    return { ...state, edition };
  }

  return state;
}

function updateLibState({ line, state }: ManifestLineInput): ManifestState {
  const field = readStringField(line);

  if (Option.isSome(field) && field.value.key === 'name') {
    const normalized = field.value.value.replaceAll('-', '_');

    return { ...state, libName: Option.some(normalized) };
  }

  return state;
}

function updateFeatureState({ line, state }: ManifestLineInput): ManifestState {
  const listMatch = LIST_FIELD_PATTERN.exec(line);
  const key = listMatch?.groups?.['key'];
  const items = listMatch?.groups?.['items'];

  if (key === undefined || items === undefined) {
    return state;
  }

  const parsedItems = readQuotedItems(items);

  const nextFeatureEntries = new Map<string, readonly string[]>([
    ...state.featureEntries,
    [key, parsedItems],
  ]);

  return { ...state, featureEntries: nextFeatureEntries };
}

function updateManifestState({ line, state }: ManifestLineInput): ManifestState {
  const nextSection = readSection(line);

  if (Option.isSome(nextSection)) {
    return { ...state, section: nextSection.value };
  }

  if (state.section === 'package') {
    return updatePackageState({ line, state });
  }

  if (state.section === 'lib') {
    return updateLibState({ line, state });
  }

  if (state.section === 'features') {
    return updateFeatureState({ line, state });
  }

  return state;
}

/** Parse package name, library name, edition, and feature entries from a Cargo.toml manifest. */
export function parseCargoManifest(toml: string): CargoManifest {
  let state: ManifestState = {
    edition: '2021',
    featureEntries: new Map<string, readonly string[]>(),
    libName: Option.none(),
    packageName: Option.none(),
    section: '',
  };

  for (const rawLine of toml.split('\n')) {
    const line = rawLine.split('#')[0] ?? '';

    state = updateManifestState({ line, state });
  }

  return {
    edition: state.edition,
    featureEntries: state.featureEntries,
    libName: state.libName,
    packageName: state.packageName,
  };
}
