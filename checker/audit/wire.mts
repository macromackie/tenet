/** Wire schemas for the `tenet audit` worker mode. */
import { Schema } from 'effect';
import type { Finding } from '../types.d.mts';

const StringList = Schema.Array(Schema.String);
const OptionalStringList = Schema.OptionFromOptionalKey(StringList);
const OptionalFlag = Schema.OptionFromOptionalKey(Schema.Boolean);
const OptionalText = Schema.OptionFromOptionalKey(Schema.String);
const AuditMode = Schema.Literal('audit');

const AuditModuleFields = Schema.Struct({
  docs: StringList,
  files: StringList,
  lowTestFiles: OptionalStringList,
  manifests: StringList,
  name: Schema.String,
  testFiles: OptionalStringList,
});

export const AuditModule = Schema.toEncoded(AuditModuleFields);
export type AuditModule = typeof AuditModule.Type;

const AuditModules = Schema.Array(AuditModule);

const AuditRequestFields = Schema.Struct({
  deep: OptionalFlag,
  fresh: OptionalFlag,
  maxRequests: Schema.Int,
  mode: AuditMode,
  modules: AuditModules,
  progressFile: OptionalText,
  root: Schema.String,
  runtimePaths: OptionalStringList,
  state: Schema.String,
});

export const AuditRequest = Schema.toEncoded(AuditRequestFields);
export type AuditRequest = typeof AuditRequest.Type;

export type AuditContractOutput = Readonly<{
  document: string;
  lockPath: string;
  message: string;
  name: string;
  paths: readonly string[];
}>;

export type AuditModuleReport = Readonly<{
  contracts: readonly AuditContractOutput[];
  costUsd: Finding['cost_usd'];
  findings: readonly Finding[];
  name: string;
  requests: number;
}>;

export type AuditResponse = Readonly<{
  costUsd: Finding['cost_usd'];
  modules: readonly AuditModuleReport[];
  requests: number;
}>;
