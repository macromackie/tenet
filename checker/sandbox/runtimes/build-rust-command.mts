import { Option } from 'effect';
import type { ProjectDeps } from '../project-deps.mts';
import type { RustCrate } from './read-rust-crate.mts';

type BuildRustCommandInput = Readonly<{
  deps: ProjectDeps;
  depsMount: string;
  linkerArg?: string;
  probeFile: string;
  probeTmp: string;
  rustCrate: Option.Option<RustCrate>;
  rustc: string;
}>;

type DepFlagsInput = Readonly<{
  deps: ProjectDeps;
  depsMount: string;
  linkerArg?: string;
}>;

const SINGLE_FILE_SCRIPT =
  'rustc="$1"; probe="$2"; out="$3"; shift 3; "$rustc" --edition=2021 --crate-name tenet_probe "$@" ./"$probe" -o "$out" && exec "$out"';

const LIB_CRATE_SCRIPT =
  'rustc="$1"; probe="$2"; out="$3"; libout="$4"; crate="$5"; edition="$6"; shift 6; "$rustc" --crate-type=lib --crate-name="$crate" --edition="$edition" "$@" ./src/lib.rs -o "$libout" && "$rustc" --edition="$edition" --crate-name=tenet_probe --extern "$crate=$libout" "$@" ./"$probe" -o "$out" && exec "$out"';

function buildDepFlags({ deps, depsMount, linkerArg }: DepFlagsInput): readonly string[] {
  const flags: string[] = [];

  if (linkerArg !== undefined) {
    flags.push('-C', `linker=${linkerArg}`);
  }

  if (deps.rustDepsDir === undefined) {
    return flags;
  }

  const externArgs = deps.rustExterns.flatMap(({ file, name }) => [
    '--extern',
    `${name}=${depsMount}/${file}`,
  ]);

  return [...flags, '-L', `dependency=${depsMount}`, ...externArgs];
}

function buildCfgFlags(features: readonly string[]): readonly string[] {
  return features.flatMap((feature) => ['--cfg', `feature="${feature}"`]);
}

/** Build a sandboxed rustc compile-and-run command, compiling `src/lib.rs` first when present. */
export function buildRustCommand({
  deps,
  depsMount,
  linkerArg,
  probeFile,
  probeTmp,
  rustCrate,
  rustc,
}: BuildRustCommandInput): readonly string[] {
  const depFlags = buildDepFlags({ deps, depsMount, linkerArg });
  const output = `${probeTmp}/observe`;

  if (Option.isNone(rustCrate)) {
    return ['/bin/sh', '-c', SINGLE_FILE_SCRIPT, 'tenet-rust', rustc, probeFile, output, ...depFlags];
  }

  const { edition, features, name } = rustCrate.value;
  const libOutput = `${probeTmp}/lib${name}.rlib`;
  const cfgFlags = buildCfgFlags(features);

  const head = [
    '/bin/sh',
    '-c',
    LIB_CRATE_SCRIPT,
    'tenet-rust',
    rustc,
    probeFile,
    output,
    libOutput,
    name,
    edition,
  ];

  return [...head, ...cfgFlags, ...depFlags];
}
