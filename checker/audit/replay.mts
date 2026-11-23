import { assertSandboxAvailable } from '../sandbox/index.mts';
import { readExecutionFiles } from './read-execution-files.mts';
import { replayTarget } from './replay-target.mts';
import type {
  AuditReplayRequest,
  AuditReplayResponse,
  AuditReplayResult,
  ReplayTarget,
} from './replay-wire.mts';

type RunAuditReplayInput = AuditReplayRequest;

function listTargetSubjects(targets: readonly ReplayTarget[]): readonly string[] {
  const subjects: string[] = [];

  for (const target of targets) {
    if (target.subject !== undefined && target.subject.length > 0) {
      subjects.push(target.subject);
    }
  }

  return subjects;
}

/** Replay audit checks directly inside the OS sandbox against the current working tree. */
export async function runAuditReplay({
  root,
  runtimePaths = [],
  state,
  targets,
}: RunAuditReplayInput): Promise<AuditReplayResponse> {
  await assertSandboxAvailable();

  const primaryPaths = listTargetSubjects(targets);
  const files = await readExecutionFiles({ primaryPaths, root, secondaryPaths: runtimePaths });
  const results: AuditReplayResult[] = [];

  for (const target of targets) {
    // Sequential replay keeps OS sandbox resource usage bounded and deterministic.
    // oxlint-disable-next-line no-await-in-loop
    const result = await replayTarget({ files, root, state, target });

    results.push(result);
  }

  return { results };
}
