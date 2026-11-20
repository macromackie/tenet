import { writeFile } from 'node:fs/promises';
import { classifyResult } from './classify-result.mts';

type ProgressInput = Readonly<{
  contract: string;
  phase: string;
  progressFile?: string;
}>;

/** Best-effort single-line phase update for interactive CLI progress indicators. */
export async function reportProgress({
  contract,
  phase,
  progressFile,
}: ProgressInput): Promise<void> {
  if (progressFile === undefined || progressFile.length === 0) {
    return;
  }

  const target = progressFile;

  async function writePhase(): Promise<void> {
    await writeFile(target, `${contract}\t${phase}\n`, 'utf-8');
  }

  await classifyResult({ message: 'progress update ignored', run: writePhase });
}
