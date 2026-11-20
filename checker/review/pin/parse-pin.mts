import { Option, Schema } from 'effect';
import { CheckerError } from '../../checker-error.mts';
import { parseBundle } from '../../schema.mts';
import type { RecordData } from '../../storage/index.mts';
import type { Inventory } from '../../types.d.mts';
import { Pin } from '../../wire.mts';
import { isMatching } from './is-matching.mts';

type ParsePinInput = {
  readonly value: Option.Option<RecordData>;
  readonly current: Inventory;
  readonly contractHash: string;
};

type LoadedPin = { matching: boolean; pin?: Pin };
const decodePin = Schema.decodeUnknownOption(Pin, { onExcessProperty: 'error' });

function readPin(record: RecordData): Pin {
  const decoded = decodePin(record.json);

  if (Option.isNone(decoded)) {
    throw new CheckerError({ message: 'the saved checker has an unsupported format' });
  }

  return decoded.value;
}

/** Validate execution inputs only when reusing the recipe for this contract and surface. */
export function parsePin({ value, current, contractHash }: ParsePinInput): LoadedPin {
  if (Option.isNone(value)) {
    return { matching: false };
  }

  const prior = readPin(value.value);

  if (!isMatching({ contractHash, current, pin: prior })) {
    return { matching: false, pin: prior };
  }

  const bundle = parseBundle({ files: current.runtimeFiles, value: prior.bundle });

  const pin = {
    bundle,
    contractHash: prior.contractHash,
    protocol: prior.protocol,
    shape: prior.shape,
  };

  return { matching: true, pin };
}
