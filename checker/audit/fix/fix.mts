import { assertSandboxAvailable } from '../../sandbox/index.mts';
import { hashSource } from '../../storage/index.mts';
import type { Finding } from '../../types.d.mts';
import { readExecutionFiles } from '../read-execution-files.mts';
import type { BudgetTracker } from '../run-round.mts';
import { repairAuditGroup } from './fix-group.mts';
import type {
  AuditFixGroup,
  AuditFixOutcome,
  AuditFixPatch,
  AuditFixRequest,
  AuditFixResponse,
} from './fix-wire.mts';

type RunAuditFixInput = AuditFixRequest;

type CollectFixPathsInput = Readonly<{
  groups: readonly AuditFixGroup[];
  runtimePaths: readonly string[];
}>;

type CollectedPaths = Readonly<{
  primaryPaths: readonly string[];
  secondaryPaths: readonly string[];
}>;

type RecordHashesInput = Readonly<{
  files: ReadonlyMap<string, string>;
  groups: readonly AuditFixGroup[];
}>;

type BuildPatchesInput = Readonly<{
  changedFilePaths: ReadonlySet<string>;
  currentFiles: ReadonlyMap<string, string>;
  originalFileHashes: ReadonlyMap<string, string>;
}>;

type ProcessGroupsInput = Readonly<{
  budget: BudgetTracker;
  deep: boolean;
  groups: readonly AuditFixGroup[];
  initialFiles: ReadonlyMap<string, string>;
  root: string;
  state: string;
}>;

type ProcessGroupsResult = Readonly<{
  changedFilePaths: ReadonlySet<string>;
  costUsd: Finding['cost_usd'];
  currentFiles: ReadonlyMap<string, string>;
  outcomes: readonly AuditFixOutcome[];
  requests: number;
}>;

type SumFixCostInput = Readonly<{
  first: Finding['cost_usd'];
  second: Finding['cost_usd'];
}>;

function createFixBudget(limit: number): BudgetTracker {
  let remaining = Math.max(0, limit);

  function consume(count: number): void {
    remaining = Math.max(0, remaining - count);
  }

  function readRemaining(): number {
    return remaining;
  }

  return { consume, readRemaining };
}

function sumFixCost({ first, second }: SumFixCostInput): Finding['cost_usd'] {
  if (first === null || second === null) {
    return null;
  }

  return first + second;
}

function collectGroupPrimaryPaths(group: AuditFixGroup): readonly string[] {
  const paths: string[] = [...group.files];

  for (const witness of group.witnesses) {
    if (witness.subject !== undefined && witness.subject.length > 0) {
      paths.push(witness.subject);
    }
  }

  for (const guard of group.guards) {
    if (guard.subject !== undefined && guard.subject.length > 0) {
      paths.push(guard.subject);
    }
  }

  return paths;
}

function collectFixPaths({ groups, runtimePaths }: CollectFixPathsInput): CollectedPaths {
  const primaryPaths: string[] = [];
  const secondaryPaths: string[] = [];

  for (const group of groups) {
    const groupPrimary = collectGroupPrimaryPaths(group);

    primaryPaths.push(...groupPrimary);
    secondaryPaths.push(...(group.relatedFiles ?? []));
  }

  secondaryPaths.push(...runtimePaths);

  return { primaryPaths, secondaryPaths };
}

function recordOriginalHashes({
  files,
  groups,
}: RecordHashesInput): ReadonlyMap<string, string> {
  const originalFileHashes = new Map<string, string>();

  for (const group of groups) {
    for (const path of group.files) {
      const content = files.get(path);

      if (content !== undefined && !originalFileHashes.has(path)) {
        const digest = hashSource(content);

        originalFileHashes.set(path, digest);
      }
    }
  }

  return originalFileHashes;
}

function buildFinalPatches({
  changedFilePaths,
  currentFiles,
  originalFileHashes,
}: BuildPatchesInput): readonly AuditFixPatch[] {
  const patches: AuditFixPatch[] = [];

  for (const path of [...changedFilePaths].toSorted()) {
    const content = currentFiles.get(path);
    const originalHash = originalFileHashes.get(path);

    if (content !== undefined && originalHash !== undefined && hashSource(content) !== originalHash) {
      patches.push({ content, originalHash, path });
    }
  }

  return patches;
}

type MergeGroupInput = Readonly<{
  fixed: Awaited<ReturnType<typeof repairAuditGroup>>;
  state: ProcessGroupsResult;
}>;

function mergeGroupResult({ fixed, state }: MergeGroupInput): ProcessGroupsResult {
  const changedFilePaths = new Set<string>([...state.changedFilePaths, ...fixed.changedPaths]);
  const costUsd = sumFixCost({ first: state.costUsd, second: fixed.costUsd });
  const outcomes = [...state.outcomes, ...fixed.outcomes];
  const requests = state.requests + fixed.requests;

  return { changedFilePaths, costUsd, currentFiles: fixed.files, outcomes, requests };
}

async function processGroups({
  budget,
  deep,
  groups,
  initialFiles,
  root,
  state: stateRoot,
}: ProcessGroupsInput): Promise<ProcessGroupsResult> {
  let state: ProcessGroupsResult = {
    changedFilePaths: new Set<string>(),
    costUsd: 0,
    currentFiles: initialFiles,
    outcomes: [],
    requests: 0,
  };

  for (const group of groups) {
    // Sequential group repair lets later groups build on earlier verified in-memory patches.
    // oxlint-disable-next-line no-await-in-loop
    const fixed = await repairAuditGroup({
      budget,
      currentFiles: state.currentFiles,
      deep,
      group,
      root,
      state: stateRoot,
    });

    state = mergeGroupResult({ fixed, state });
  }

  return state;
}

/** Generate and sandbox-verify minimal in-memory repair patches across audit witness groups. */
export async function runAuditFix({
  deep = false,
  groups,
  maxRequests,
  root,
  runtimePaths = [],
  state,
}: RunAuditFixInput): Promise<AuditFixResponse> {
  await assertSandboxAvailable();

  const { primaryPaths, secondaryPaths } = collectFixPaths({ groups, runtimePaths });
  const initialFiles = await readExecutionFiles({ primaryPaths, root, secondaryPaths });
  const originalFileHashes = recordOriginalHashes({ files: initialFiles, groups });
  const budget = createFixBudget(maxRequests);
  const processed = await processGroups({ budget, deep, groups, initialFiles, root, state });

  const patches = buildFinalPatches({
    changedFilePaths: processed.changedFilePaths,
    currentFiles: processed.currentFiles,
    originalFileHashes,
  });

  return {
    costUsd: processed.costUsd,
    outcomes: processed.outcomes,
    patches,
    requests: processed.requests,
  };
}
