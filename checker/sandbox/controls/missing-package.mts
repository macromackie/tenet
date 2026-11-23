import { basename, extname } from 'node:path';
import { Option } from 'effect';
import type { Check } from '../../types.d.mts';
import type { ProjectDeps } from '../project-deps.mts';

const SCOPED_PACKAGE_PARTS = 2;

const PYTHON_PATTERNS = [
  /ModuleNotFoundError:\s*No module named ['"](?<pkg>[^'"]+)['"]/v,
  /ImportError:\s*No module named ['"]?(?<pkg>[A-Za-z0-9_.\-]+)['"]?/v,
  /ImportError:[^\n]*(?:missing optional dependency|requires)\s+['"](?<pkg>[A-Za-z0-9_.\-]+)['"]/v,
] as const;

const NODE_PATTERNS = [
  /Cannot find (?:package|module) ['"](?<pkg>[^'"]+)['"]/v,
  /ERR_MODULE_NOT_FOUND[^\n]*['"](?<pkg>[^'"]+)['"]/v,
] as const;

const GO_PATTERNS = [
  /no required module provides package (?<pkg>[^\s:;]+)/v,
  /cannot find package "(?<pkg>[^"]+)"/v,
  /missing go\.sum entry for module providing package (?<pkg>[^\s:;]+)/v,
] as const;

const RUST_PATTERNS = [
  /error\[E0463\]:[^\n]*can't find crate for `(?<pkg>[A-Za-z0-9_\-]+)`/v,
  /error\[E0432\]:[^\n]*unresolved import `(?<pkg>[A-Za-z0-9_\-]+)(?:::|`)/v,
] as const;

const GO_MODULE_PATTERN = /^module\s+(?<mod>\S+)/mv;
const NON_BARE_PREFIXES = ['.', '/', 'file:', 'node:'] as const;

const RUST_BUILTIN_CRATES: ReadonlySet<string> = new Set([
  'alloc',
  'core',
  'crate',
  'self',
  'std',
  'super',
]);

const CONTAINER_DIRS: ReadonlySet<string> = new Set(['lib', 'packages', 'pkg', 'src']);

type MissingPackageInput = Readonly<{
  deps?: ProjectDeps;
  filePaths?: readonly string[];
  runtime?: Check['runtime'];
  stderr: string;
}>;

type MatchPatternsInput = Readonly<{ lines: readonly string[]; patterns: readonly RegExp[] }>;
type MatchLineInput = Readonly<{ line: string; patterns: readonly RegExp[] }>;
type LocalGoInput = Readonly<{ deps?: ProjectDeps; pkg: string }>;
type LocalFilterInput = Readonly<{ localModules: ReadonlySet<string>; pkg: string }>;

type FilterPackageInput = Readonly<{
  deps?: ProjectDeps;
  localModules: ReadonlySet<string>;
  pkg: string;
  runtime: Check['runtime'];
}>;

function readStem(name: string): string {
  const ext = extname(name);

  if (ext.length === 0) {
    return name;
  }

  return name.slice(0, -ext.length);
}

function readDirModules(path: string): readonly string[] {
  const segments = path.split('/');
  const first = segments.at(0);
  const second = segments.at(1);
  const dirs: string[] = [];

  if (first !== undefined && first.length > 0) {
    dirs.push(first);
  }

  if (first !== undefined && CONTAINER_DIRS.has(first) && second !== undefined) {
    dirs.push(second);
  }

  return dirs;
}

function readPathModules(path: string): readonly string[] {
  const dirs = readDirModules(path);
  const name = basename(path);
  const stem = readStem(name);

  if (stem.length === 0) {
    return dirs;
  }

  const normalizedStem = stem.replaceAll('-', '_');

  return [...dirs, stem, normalizedStem];
}

function collectLocalModules(filePaths?: readonly string[]): ReadonlySet<string> {
  const localModules = new Set<string>();

  if (filePaths === undefined) {
    return localModules;
  }

  for (const path of filePaths) {
    for (const mod of readPathModules(path)) {
      localModules.add(mod);
    }
  }

  return localModules;
}

function matchLine({ line, patterns }: MatchLineInput): Option.Option<string> {
  for (const pattern of patterns) {
    const match = pattern.exec(line);
    const pkg = match?.groups?.['pkg']?.trim();

    if (pkg !== undefined && pkg.length > 0) {
      return Option.some(pkg);
    }
  }

  return Option.none();
}

function matchLastPattern({ lines, patterns }: MatchPatternsInput): Option.Option<string> {
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const line = lines.at(index) ?? '';
    const found = matchLine({ line, patterns });

    if (Option.isSome(found)) {
      return found;
    }
  }

  return Option.none();
}

function isNonBareSpecifier(specifier: string): boolean {
  return NON_BARE_PREFIXES.some((prefix) => specifier.startsWith(prefix));
}

function readNodePackageName(specifier: string): Option.Option<string> {
  if (isNonBareSpecifier(specifier)) {
    return Option.none();
  }

  const parts = specifier.split('/');
  const head = parts.at(0) ?? '';
  const second = parts.at(1) ?? '';

  if (specifier.startsWith('@') && parts.length >= SCOPED_PACKAGE_PARTS) {
    return Option.some(`${head}/${second}`);
  }

  if (head.length === 0) {
    return Option.none();
  }

  return Option.some(head);
}

function readGoModulePath(goMod?: string): string {
  if (goMod === undefined) {
    return '';
  }

  const match = GO_MODULE_PATTERN.exec(goMod);

  return match?.groups?.['mod']?.trim() ?? '';
}

function isLocalGoPackage({ deps, pkg }: LocalGoInput): boolean {
  if (pkg.startsWith('.') || pkg === 'tenet_check' || pkg.startsWith('tenet_check/')) {
    return true;
  }

  const modulePath = readGoModulePath(deps?.goMod);

  if (modulePath.length === 0) {
    return false;
  }

  return pkg === modulePath || pkg.startsWith(`${modulePath}/`);
}

function filterPythonPackage({ localModules, pkg }: LocalFilterInput): Option.Option<string> {
  const root = pkg.split('.').at(0) ?? '';

  if (pkg.startsWith('.') || root.length === 0 || localModules.has(root)) {
    return Option.none();
  }

  return Option.some(root);
}

function filterNodePackage({ localModules, pkg }: LocalFilterInput): Option.Option<string> {
  const bare = readNodePackageName(pkg);

  if (Option.isNone(bare) || localModules.has(bare.value)) {
    return Option.none();
  }

  return bare;
}

function filterGoPackage({ deps, pkg }: LocalGoInput): Option.Option<string> {
  if (isLocalGoPackage({ deps, pkg })) {
    return Option.none();
  }

  return Option.some(pkg);
}

function filterRustPackage({ localModules, pkg }: LocalFilterInput): Option.Option<string> {
  if (RUST_BUILTIN_CRATES.has(pkg) || localModules.has(pkg)) {
    return Option.none();
  }

  return Option.some(pkg);
}

function filterExternalPackage({
  deps,
  localModules,
  pkg,
  runtime,
}: FilterPackageInput): Option.Option<string> {
  switch (runtime) {
    case 'python': {
      return filterPythonPackage({ localModules, pkg });
    }
    case 'node': {
      return filterNodePackage({ localModules, pkg });
    }
    case 'go': {
      return filterGoPackage({ deps, pkg });
    }
    case 'rust': {
      return filterRustPackage({ localModules, pkg });
    }
  }
}

function selectRuntimePatterns(runtime: Check['runtime']): readonly RegExp[] {
  switch (runtime) {
    case 'python': {
      return PYTHON_PATTERNS;
    }
    case 'node': {
      return NODE_PATTERNS;
    }
    case 'go': {
      return GO_PATTERNS;
    }
    case 'rust': {
      return RUST_PATTERNS;
    }
  }
}

/** Detect an uninstalled third-party package from probe stderr while ignoring repo-local typos. */
export function detectMissingExternalPackage({
  deps,
  filePaths,
  runtime,
  stderr,
}: MissingPackageInput): Option.Option<string> {
  if (runtime === undefined) {
    return Option.none();
  }

  const lines = stderr.split('\n');
  const patterns = selectRuntimePatterns(runtime);
  const rawMatch = matchLastPattern({ lines, patterns });

  if (Option.isNone(rawMatch)) {
    return Option.none();
  }

  const localModules = collectLocalModules(filePaths);

  return filterExternalPackage({ deps, localModules, pkg: rawMatch.value, runtime });
}
