import type { Check } from '../types.d.mts';

type BuildCheckEntryInput = Readonly<{
  check: Check;
  scriptFile: string;
  scriptHash?: string;
  test?: string;
}>;

type BaseCheck = Pick<
  Check,
  'brownfield' | 'defect' | 'expected' | 'good' | 'id' | 'promise' | 'refactor' | 'runtime' | 'subject'
>;

type AttachScriptMetaInput = Readonly<{
  base: BaseCheck;
  scriptFile: string;
  scriptHash?: string;
}>;

function readCheckTarget(check: Check): Pick<Check, 'expected' | 'id' | 'promise' | 'runtime' | 'subject'> {
  return {
    expected: check.expected,
    id: check.id,
    promise: check.promise,
    runtime: check.runtime,
    subject: check.subject,
  };
}

function readCheckControls(check: Check): Pick<Check, 'brownfield' | 'defect' | 'good' | 'refactor'> {
  if (check.brownfield !== undefined) {
    return {
      brownfield: check.brownfield,
      defect: check.defect,
      good: check.good,
      refactor: check.refactor,
    };
  }

  return { defect: check.defect, good: check.good, refactor: check.refactor };
}

function attachScriptMeta({ base, scriptFile, scriptHash }: AttachScriptMetaInput): Check {
  if (scriptHash !== undefined) {
    return { ...base, scriptFile, scriptHash };
  }

  return { ...base, scriptFile };
}

/** Construct a compact on-disk or hydrated in-memory Check entry with canonical key ordering. */
export function buildCheckEntry({
  check,
  scriptFile,
  scriptHash,
  test,
}: BuildCheckEntryInput): Check {
  const target = readCheckTarget(check);
  const controls = readCheckControls(check);
  const entry = attachScriptMeta({ base: { ...target, ...controls }, scriptFile, scriptHash });

  if (test !== undefined) {
    return { ...entry, test };
  }

  return entry;
}
