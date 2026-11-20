import { basename } from 'node:path';
import type { Contract } from '../types.d.mts';
import { MANIFESTS } from './manifests.mts';

const enumRank = {
  failure: 1,
  manifest: 0,
  otherPublic: 3,
  privateOrTest: 4,
  publicSource: 2,
  unrelated: 5,
} as const;

type RankPathInput = Readonly<{
  decision?: string;
  path: string;
}>;

function isPrivateModule(path: string): boolean {
  const name = basename(path);

  return name.startsWith('_') && name !== '__init__.py';
}

function isTestPath(path: string): boolean {
  const segments = path.split('/');

  return segments.includes('test') || segments.includes('tests');
}

function rankUnclassifiedPath(path: string): number {
  if (isPrivateModule(path) || isTestPath(path)) {
    return enumRank.privateOrTest;
  }

  if (path.startsWith('src/') || path.startsWith('lib/')) {
    return enumRank.publicSource;
  }

  return enumRank.otherPublic;
}

function rankPath({ path, decision }: RankPathInput): number {
  const name = basename(path);

  if (MANIFESTS.has(name)) {
    return enumRank.manifest;
  }

  if (decision === 'fail') {
    return enumRank.failure;
  }

  if (decision === 'not_applicable') {
    return enumRank.unrelated;
  }

  return rankUnclassifiedPath(path);
}

/** Order governed paths so manifests, flagged files and public sources fit ahead of internals. */
export function listRankedPaths(contract: Contract): readonly string[] {
  const paths = contract.paths.toSorted();

  const groups = [
    enumRank.manifest,
    enumRank.failure,
    enumRank.publicSource,
    enumRank.otherPublic,
    enumRank.privateOrTest,
    enumRank.unrelated,
  ];

  return groups.flatMap((group) =>
    paths.filter((path) => rankPath({ decision: contract.decisions?.[path], path }) === group),
  );
}
