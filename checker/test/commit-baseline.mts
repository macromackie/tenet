import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { SOURCE } from './test-project.mts';

// Node also returns the ChildProcess in its callback overload.
// oxlint-disable-next-line typescript/strict-void-return
const execute = promisify(execFile);

/** Record the defective source as the base revision for a diff review. */
export async function commitBaseline(root: string): Promise<string> {
  async function runGit(args: readonly string[]) {
    return await execute('git', ['-C', root, ...args]);
  }

  await runGit(['init', '-q']);
  await runGit(['add', SOURCE]);
  await runGit([
    '-c',
    'user.name=Test',
    '-c',
    'user.email=test@example.org',
    'commit',
    '-qm',
    'base',
  ]);

  const result = await runGit(['rev-parse', 'HEAD']);

  return result.stdout.trim();
}
