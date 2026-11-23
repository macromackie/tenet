/** Wire schemas for the `tenet audit` replay worker mode. */
import { Schema } from 'effect';

const StringList = Schema.Array(Schema.String);
const OptionalStringList = Schema.OptionFromOptionalKey(StringList);
const OptionalText = Schema.OptionFromOptionalKey(Schema.String);
const ReplayMode = Schema.Literal('replay');

const ReplayReproFields = Schema.Struct({
  code: Schema.String,
  expected: Schema.String,
  observed: OptionalText,
  runtime: Schema.String,
});

export const ReplayRepro = Schema.toEncoded(ReplayReproFields);
export type ReplayRepro = typeof ReplayRepro.Type;

const OptionalReplayRepro = Schema.OptionFromOptionalKey(ReplayRepro);

const ReplayTargetFields = Schema.Struct({
  check: Schema.String,
  contract: Schema.String,
  lockPath: OptionalText,
  repro: OptionalReplayRepro,
  subject: OptionalText,
});

const ReplayTarget = Schema.toEncoded(ReplayTargetFields);
export type ReplayTarget = typeof ReplayTarget.Type;

const ReplayTargetList = Schema.Array(ReplayTarget);

const AuditReplayRequestFields = Schema.Struct({
  mode: ReplayMode,
  root: Schema.String,
  runtimePaths: OptionalStringList,
  state: Schema.String,
  targets: ReplayTargetList,
});

export const AuditReplayRequest = Schema.toEncoded(AuditReplayRequestFields);
export type AuditReplayRequest = typeof AuditReplayRequest.Type;

export type AuditReplayResult = Readonly<{
  check: string;
  contract: string;
  expected?: string;
  message: string;
  observed?: string;
  outcome: 'passed' | 'failed' | 'unresolved';
  subject?: string;
}>;

export type AuditReplayResponse = Readonly<{
  results: readonly AuditReplayResult[];
}>;
