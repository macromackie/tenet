/** Wire schemas for the `tenet audit --fix` worker mode. */
import { Schema } from 'effect';
import type { Finding } from '../../types.d.mts';
import { OptionalPinRoot, ReplayRepro } from '../replay-wire.mts';

const StringList = Schema.Array(Schema.String);
const OptionalStringList = Schema.OptionFromOptionalKey(StringList);
const OptionalFlag = Schema.OptionFromOptionalKey(Schema.Boolean);
const OptionalText = Schema.OptionFromOptionalKey(Schema.String);
const OptionalReplayRepro = Schema.OptionFromOptionalKey(ReplayRepro);
const FixMode = Schema.Literal('audit-fix');

const AuditFixContractFields = Schema.Struct({
  document: Schema.String,
  lockPath: Schema.String,
  message: Schema.String,
  name: Schema.String,
  paths: StringList,
});

const AuditFixContract = Schema.toEncoded(AuditFixContractFields);
const AuditFixContractList = Schema.Array(AuditFixContract);

const AuditFixGuardFields = Schema.Struct({
  check: Schema.String,
  contract: Schema.String,
  lockPath: OptionalText,
  pinRoot: OptionalPinRoot,
  repro: OptionalReplayRepro,
  subject: OptionalText,
});

const AuditFixGuard = Schema.toEncoded(AuditFixGuardFields);
const AuditFixGuardList = Schema.Array(AuditFixGuard);

const AuditFixWitnessFields = Schema.Struct({
  check: Schema.String,
  contract: Schema.String,
  lockPath: OptionalText,
  message: Schema.String,
  pinRoot: OptionalPinRoot,
  repro: OptionalReplayRepro,
  subject: OptionalText,
});

const AuditFixWitness = Schema.toEncoded(AuditFixWitnessFields);
export type AuditFixWitness = typeof AuditFixWitness.Type;

const AuditFixWitnessList = Schema.Array(AuditFixWitness);

const AuditFixGroupFields = Schema.Struct({
  contracts: AuditFixContractList,
  files: StringList,
  guards: AuditFixGuardList,
  name: Schema.String,
  relatedFiles: OptionalStringList,
  testFiles: OptionalStringList,
  witnesses: AuditFixWitnessList,
});

const AuditFixGroup = Schema.toEncoded(AuditFixGroupFields);
export type AuditFixGroup = typeof AuditFixGroup.Type;

const AuditFixGroupList = Schema.Array(AuditFixGroup);

const AuditFixRequestFields = Schema.Struct({
  deep: OptionalFlag,
  groups: AuditFixGroupList,
  maxRequests: Schema.Int,
  mode: FixMode,
  progressFile: OptionalText,
  root: Schema.String,
  runtimePaths: OptionalStringList,
  state: Schema.String,
});

export const AuditFixRequest = Schema.toEncoded(AuditFixRequestFields);
export type AuditFixRequest = typeof AuditFixRequest.Type;

export type AuditFixOutcomeStatus = 'already_passing' | 'fixed' | 'failed' | 'unresolved';

export type AuditFixOutcome = Readonly<{
  check: string;
  contract: string;
  expected?: string;
  message: string;
  observed?: string;
  status: AuditFixOutcomeStatus;
  subject?: string;
}>;

export type AuditFixPatch = Readonly<{
  content: string;
  originalHash: string;
  path: string;
}>;

export type AuditFixResponse = Readonly<{
  costUsd: Finding['cost_usd'];
  outcomes: readonly AuditFixOutcome[];
  patches: readonly AuditFixPatch[];
  requests: number;
}>;
