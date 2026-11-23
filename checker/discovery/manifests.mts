export const MANIFESTS: ReadonlySet<string> = new Set([
  'package.json',
  'Cargo.toml',
  'go.mod',
  'go.sum',
  'pyproject.toml',
  'tsconfig.json',
]);

export const EXTENSIONS: ReadonlySet<string> = new Set([
  '.js',
  '.cjs',
  '.mjs',
  '.ts',
  '.mts',
  '.cts',
  '.tsx',
  '.jsx',
  '.py',
  '.rs',
  '.go',
]);

export const UNSUPPORTED_EXTENSIONS: ReadonlySet<string> = new Set([
  '.java',
  '.c',
  '.cc',
  '.cpp',
  '.swift',
  '.rb',
  '.cs',
  '.kt',
]);

export const PACKAGE_DATA_EXTENSIONS: ReadonlySet<string> = new Set(['.txt', '.json', '.csv']);
