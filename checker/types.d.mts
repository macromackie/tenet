export type {
  Bundle,
  Check,
  Contract,
  Finding,
  Pin,
  Receipt,
  Request,
  WitnessRepro,
} from './wire.mts';

export type Inventory = {
  readonly assets: Readonly<Record<string, string>>;
  readonly decisions: Readonly<Record<string, string>>;
  readonly files: ReadonlyMap<string, string>;
  readonly observed: string;
  readonly runtimeFiles: ReadonlyMap<string, string>;
  readonly omitted: readonly string[];
  readonly shape: string;
};
