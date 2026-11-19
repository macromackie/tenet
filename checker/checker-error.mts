import { Data } from 'effect';

/** An unavailable checker or incomplete observation, with its original cause when available. */
export class CheckerError extends Data.TaggedError('CheckerError')<{
  readonly message: string;
  readonly cause?: unknown;
}> {}
