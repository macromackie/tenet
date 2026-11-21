const HOOK_LINES = [
  'import { statSync } from "node:fs";',
  'import * as mod from "node:module";',
  'import { dirname, resolve as resolvePath, sep } from "node:path";',
  'import { fileURLToPath, pathToFileURL } from "node:url";',
  'const SUFFIXES = [".js", ".mjs", ".cjs", ".ts", ".mts", ".cts", "/index.js", "/index.mjs", "/index.cjs", "/index.ts", "/index.mts", "/index.cts"];',
  'if (typeof mod.registerHooks === "function") {',
  '  const root = process.cwd();',
  '  mod.registerHooks({',
  '    resolve(specifier, context, nextResolve) {',
  '      try {',
  '        return nextResolve(specifier, context);',
  '      } catch (error) {',
  '        if ((specifier.startsWith("./") || specifier.startsWith("../")) && context.parentURL?.startsWith("file://")) {',
  '          const base = resolvePath(dirname(fileURLToPath(context.parentURL)), specifier);',
  '          if (base === root || base.startsWith(root + sep)) {',
  '            for (const suffix of SUFFIXES) {',
  '              const candidate = base + suffix;',
  '              if (statSync(candidate, { throwIfNoEntry: false })?.isFile()) {',
  '                return { shortCircuit: true, url: pathToFileURL(candidate).href };',
  '              }',
  '            }',
  '          }',
  '        }',
  '        throw error;',
  '      }',
  '    },',
  '  });',
  '}',
] as const;

const HOOK_SOURCE = HOOK_LINES.join('\n');
const HOOK_DATA_URL = `data:text/javascript,${encodeURIComponent(HOOK_SOURCE)}`;

/** Build the `--import` arguments enabling bounded extensionless relative ESM resolution. */
export function buildNodeResolveHookFlags(): readonly string[] {
  return ['--import', HOOK_DATA_URL];
}
