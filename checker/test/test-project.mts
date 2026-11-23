import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { TestContext } from 'node:test';

const DOCUMENT = 'An explicit value takes precedence over the default.';
export const FOLDER = '.contracts/001-options';
export const lock = join(FOLDER, 'scripts', '.lock.json');
export const SOURCE = 'choose.mjs';
export const GOOD = 'export const choose = (value) => value ?? 7;';
const DEFECT = 'export const choose = (value) => value || 7;';

export const contract = {
  document: DOCUMENT,
  hash: 'test-contract',
  name: 'options',
  path: join(FOLDER, 'CONTRACT.md'),
  paths: [SOURCE],
} as const;

export const check = {
  defect: DEFECT,
  expected: 0,
  good: GOOD,
  id: 'keeps-zero',
  promise: DOCUMENT,
  refactor: 'export const choose = (n) => n == null ? 7 : n;',
  runtime: 'node',
  subject: SOURCE,
  test: 'import {choose} from "./choose.mjs"; console.log(JSON.stringify(choose(0)));',
} as const;

async function initialize(root: string) {
  const contractFolder = join(root, FOLDER);

  await mkdir(contractFolder, { recursive: true });

  const documentPath = join(root, contract.path);

  await writeFile(documentPath, DOCUMENT);

  const sourcePath = join(root, SOURCE);

  await writeFile(sourcePath, DEFECT);
}

/** Create a disposable repository containing a defective explicit-value implementation. */
export async function createProject(context: TestContext): Promise<string> {
  const directory = tmpdir();
  const prefix = join(directory, 'tenet-replay-');
  const root = await mkdtemp(prefix);

  context.after(async () => {
    await rm(root, { force: true, recursive: true });
  });

  await initialize(root);

  return root;
}
