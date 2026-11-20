/** JSON representations shared with the host, local receipts and portable pins. */
import { Schema } from 'effect';

const Paths = Schema.Array(Schema.String);
const OptionalPaths = Schema.OptionFromOptionalKey(Paths);
const OptionalText = Schema.OptionFromOptionalKey(Schema.String);
const NullableText = Schema.OptionFromNullOr(Schema.String);
const FileDecisions = Schema.Record(Schema.String, Schema.String);
const Outcome = Schema.Literals(['passed', 'failed', 'unresolved']);

const NullableCost = Schema.OptionFromNullOr(Schema.Number);
const OptionalCount = Schema.OptionFromOptionalKey(Schema.Int);

const FindingFields = Schema.Struct({
  check: Schema.String,
  contract: Schema.String,
  cost_usd: NullableCost,
  generation_requests: OptionalCount,
  message: Schema.String,
  outcome: Outcome,
  requested: Schema.Boolean,
  reused: Schema.Boolean,
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
});

export const Contract = Schema.toEncoded(ContractFields);
export type Contract = typeof Contract.Type;

const OptionalBase = Schema.OptionFromOptionalNullOr(Schema.String);
const ContractList = Schema.Array(Contract);
const OptionalFlag = Schema.OptionFromOptionalKey(Schema.Boolean);

const RequestFields = Schema.Struct({
  base: OptionalBase,
  contracts: ContractList,
  fresh: OptionalFlag,
  inspect: OptionalFlag,
  maxGenerations: Schema.Int,
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

const Runtime = Schema.Literals(['node', 'python', 'rust']);

const CheckFields = Schema.Struct({
  defect: Schema.String,
  expected: Schema.Unknown,
  good: Schema.String,
  id: Schema.String,
  promise: Schema.String,
  refactor: Schema.String,
  runtime: Runtime,
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
