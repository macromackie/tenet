/** JSON representations shared with the host, local receipts and portable pins. */
import { Schema } from 'effect';

const Paths = Schema.Array(Schema.String);
const OptionalPaths = Schema.OptionFromOptionalKey(Paths);
const OptionalText = Schema.OptionFromOptionalKey(Schema.String);
const NullableText = Schema.OptionFromNullOr(Schema.String);
const FileDecisions = Schema.Record(Schema.String, Schema.String);
const Outcome = Schema.Literals(['passed', 'failed', 'unresolved']);

const NullableCost = Schema.OptionFromNullOr(Schema.Number);
const OptionalNullableCost = Schema.OptionFromOptionalKey(NullableCost);
const OptionalCount = Schema.OptionFromOptionalKey(Schema.Int);
const OptionalFlag = Schema.OptionFromOptionalKey(Schema.Boolean);
const Runtime = Schema.Literals(['node', 'python', 'rust', 'go']);
const Severity = Schema.Literals(['critical', 'high', 'medium']);
const OptionalSeverity = Schema.OptionFromOptionalKey(Severity);

const WitnessReproFields = Schema.Struct({
  code: Schema.String,
  expected: Schema.String,
  observed: OptionalText,
  runtime: Runtime,
});

const WitnessRepro = Schema.toEncoded(WitnessReproFields);
export type WitnessRepro = typeof WitnessRepro.Type;

const OptionalRepro = Schema.OptionFromOptionalKey(WitnessRepro);

const FindingFields = Schema.Struct({
  brownfield_unverified: OptionalFlag,
  check: Schema.String,
  contract: Schema.String,
  cost_usd: NullableCost,
  explanation_cost_usd: OptionalNullableCost,
  explanation_requests: OptionalCount,
  generation_requests: OptionalCount,
  message: Schema.String,
  outcome: Outcome,
  pin_reused: OptionalFlag,
  repro: OptionalRepro,
  requested: Schema.Boolean,
  reused: Schema.Boolean,
  severity: OptionalSeverity,
  subject: NullableText,
});

export const Finding = Schema.toEncoded(FindingFields);
export type Finding = typeof Finding.Type;

const Findings = Schema.Array(Finding);

const OptionalFileDecisions = Schema.OptionFromOptionalNullOr(FileDecisions);
const OptionalFindings = Schema.OptionFromOptionalKey(Findings);

const ContractFields = Schema.Struct({
  assets: OptionalPaths,
  checks: OptionalPaths,
  decisions: OptionalFileDecisions,
  document: Schema.String,
  hash: Schema.String,
  mechanical: OptionalFindings,
  name: Schema.String,
  path: Schema.String,
  paths: Paths,
  runtimePaths: OptionalPaths,
});

export const Contract = Schema.toEncoded(ContractFields);
export type Contract = typeof Contract.Type;

const OptionalBase = Schema.OptionFromOptionalNullOr(Schema.String);
const ContractList = Schema.Array(Contract);

const RequestFields = Schema.Struct({
  allowBrownfieldWitness: OptionalFlag,
  base: OptionalBase,
  contracts: ContractList,
  fresh: OptionalFlag,
  inspect: OptionalFlag,
  maxGenerations: Schema.Int,
  pinRoot: OptionalText,
  progressFile: OptionalText,
  root: Schema.String,
  state: Schema.String,
});

export const Request = Schema.toEncoded(RequestFields);
export type Request = typeof Request.Type;

const ReceiptFields = Schema.Struct({
  base: NullableText,
  observed: Schema.String,
  qualifications: Paths,
  rows: Findings,
  shape: Schema.String,
  signature: OptionalText,
  tools: Schema.String,
});

export const Receipt = Schema.toEncoded(ReceiptFields);
export type Receipt = typeof Receipt.Type;

const CheckFields = Schema.Struct({
  brownfield: OptionalFlag,
  defect: Schema.String,
  expected: Schema.Unknown,
  good: Schema.String,
  id: Schema.String,
  promise: Schema.String,
  refactor: Schema.String,
  runtime: Runtime,
  scriptFile: OptionalText,
  scriptHash: OptionalText,
  subject: Schema.String,
  test: OptionalText,
  testFile: OptionalText,
});

export const Check = Schema.toEncoded(CheckFields);
export type Check = typeof Check.Type;

const CheckList = Schema.Array(Check);
export const Bundle = Schema.Struct({ checks: CheckList, open: Paths });
export type Bundle = typeof Bundle.Type;

const PinProtocol = Schema.Literal(1);

const PinFields = Schema.Struct({
  bundle: Bundle,
  contractHash: OptionalText,
  protocol: PinProtocol,
  shape: Schema.String,
});

export const Pin = Schema.toEncoded(PinFields);
export type Pin = typeof Pin.Type;
