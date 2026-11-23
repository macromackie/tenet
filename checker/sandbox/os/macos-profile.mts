import type { ProjectDeps } from '../project-deps.mts';

type SeatbeltProfileInput = Readonly<{
  deps: ProjectDeps;
  probeTmp: string;
  runtimeRoot: string;
  work: string;
}>;

function escapeScheme(value: string): string {
  return value.replaceAll('\\', String.raw`\\`).replaceAll('"', String.raw`\"`);
}

function collectReadOnlyDepPaths(deps: ProjectDeps): readonly string[] {
  const paths: string[] = [];

  const candidates = [
    deps.goModCache,
    deps.goVendor,
    deps.nodeModules,
    deps.pythonSitePackages,
    deps.rustBuildOutDir,
    deps.rustDepsDir,
  ];

  for (const entry of candidates) {
    if (entry !== undefined) {
      paths.push(entry);
    }
  }

  return paths;
}

/** Build a macOS Seatbelt profile allowing read-only source and dependency access with no network. */
export function buildSeatbeltProfile({
  deps,
  probeTmp,
  runtimeRoot,
  work,
}: SeatbeltProfileInput): string {
  const allowedReadPaths = [runtimeRoot, work, probeTmp, ...collectReadOnlyDepPaths(deps)];
  const readRules = allowedReadPaths.map((entry) => `(subpath "${escapeScheme(entry)}")`).join(' ');
  const safeTmp = escapeScheme(probeTmp);

  return [
    '(version 1)',
    '(deny default)',
    '(allow process-fork process-exec)',
    '(allow signal (target same-sandbox))',
    '(allow process-info* (target same-sandbox))',
    '(allow sysctl-read)',
    '(allow mach-lookup)',
    '(allow file-read*)',
    '(allow file-map-executable)',
    '(deny file-read-data (subpath "/Users") (subpath "/Volumes") (subpath "/private/tmp") (subpath "/private/var/folders"))',
    `(allow file-read-data ${readRules})`,
    `(allow file-write* (subpath "${safeTmp}") (literal "/dev/null") (literal "/dev/zero"))`,
    '(deny network*)',
  ].join('\n');
}
