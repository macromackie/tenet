import { basename } from 'node:path';
import { hashSource } from '../storage/index.mts';
import type { Contract } from '../types.d.mts';
import { MANIFESTS } from './manifests.mts';

const SURFACE_VERSION = 2;

const CONTROL_WORDS: ReadonlySet<string> = new Set([
  'if',
  'switch',
  'for',
  'while',
  'catch',
  'function',
]);

const DECLARATION =
  /(?:^|\n)\s*(?:export\s+(?:(?:default|async)\s+)*(?:class|function|const|let|enum|type|interface)?\s*|(?:pub\s+(?:async\s+)?(?:fn|struct|trait|enum|type|mod)|(?:async\s+)?def|class)\s+)(?<name>[\w$]+)/gv;

const MEMBER =
  /(?:^|[\{\};\n])\s*(?:(?:public|static|async|readonly|abstract|get|set)\s+)*(?<name>[A-Za-z_$][\w$]*)\s*\([^\(\)\{\};]*\)\s*(?::[^=\{\};]+)?\s*\{/gv;

const EXPORT =
  /(?:^|\n)\s*(?:export\s+(?:type\s+)?\{[^\}]+\}(?:\s+from\s+[^;\n]+)?|export\s+\*\s+from\s+[^;\n]+|pub\s+use\s+[^;]+);?/gv;

function readSurface([path, source]: readonly [string, string]): readonly [
  string,
  string | readonly string[],
] {
  const name = basename(path);

  if (MANIFESTS.has(name)) {
    return [path, hashSource(source)];
  }

  const declarationMatches = source.matchAll(DECLARATION);
  const declared = Array.from(declarationMatches, (match) => match[1]);

  const memberMatches = source.matchAll(MEMBER);

  const members = Array.from(memberMatches, (match) => match[1]).filter(
    (member) => !CONTROL_WORDS.has(member),
  );

  const exportMatches = source.matchAll(EXPORT);
  const exported = Array.from(exportMatches, (match) => match[0].trim());

  return [path, [...declared, ...members, ...exported].toSorted()];
}

type HashSurfaceInput = {
  contract: Contract;
  source: readonly [string, string][];
  assets: Record<string, string>;
  omitted: readonly string[];
};

/** Key checker discovery by contract, declared surface, reference samples and known gaps. */
export function hashSurface({ contract, source, assets, omitted }: HashSurfaceInput): string {
  const names = source.map((entry) => readSurface(entry));
  const hints = Object.entries(assets);
  const missing = omitted.toSorted();

  const identity = JSON.stringify([SURFACE_VERSION, contract.hash, names, hints, missing]);

  return hashSource(identity);
}
