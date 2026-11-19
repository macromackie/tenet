#!/usr/bin/env bash
# The published Rust crate embeds this standalone program so replay needs no npm install.
set -euo pipefail
cd -- "$(dirname -- "$0")"
scratch="$(mktemp -d)"
trap 'rm -rf -- "${scratch}"' EXIT
node_modules/.bin/esbuild worker.mts --bundle --format=esm --platform=node --target=node22 \
  --charset=utf8 --legal-comments=eof --external:libfx --outfile="${scratch}/worker.mjs" \
  --metafile="${scratch}/inputs.json" --log-level=warning

node --input-type=module - "${scratch}/inputs.json" >> "${scratch}/worker.mjs" <<'JS'
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const metadata = JSON.parse(readFileSync(process.argv[2], 'utf8'));
const packages = new Set();
for (const file of Object.keys(metadata.inputs)) {
  const match = file.match(/^(.*node_modules\/(?:@[^/]+\/)?[^/]+)\//);
  if (match) packages.add(match[1]);
}
for (const folder of [...packages].sort()) {
  const name = JSON.parse(readFileSync(join(folder, 'package.json'), 'utf8')).name;
  const license = ['LICENSE', 'LICENSE.md', 'LICENSE.txt'].map(path => join(folder, path)).find(existsSync);
  if (!license) throw new Error('Missing license for ' + name);
  const text = readFileSync(license, 'utf8').replaceAll('*/', '* /');
  process.stdout.write('\n/*! ' + name + '\n' + text + '\n*/\n');
}
JS

if [[ "${1:-}" == '--check' ]]; then
  cmp worker.bundle "${scratch}/worker.mjs"
else
  mv -- "${scratch}/worker.mjs" worker.bundle
fi
