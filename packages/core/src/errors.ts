export type ErrorCode =
  | 'Unsupported'
  | 'Corrupt'
  | 'Encrypted'
  | 'TooLarge'
  | 'Timeout'
  | 'UpstreamFailed'
  | 'Internal';

/** Typed engine/core error. `why` and `todo` feed the user message; `details` is the raw engine text. */
export class RefractError extends Error {
  constructor(
    readonly code: ErrorCode,
    readonly why: string,
    readonly todo: string,
    readonly details?: string,
  ) {
    super(`${code}: ${why}`);
    this.name = 'RefractError';
  }
}

/** "What happened, why, what to do." */
export function describeError(fileName: string, err: RefractError): string {
  return `Couldn't convert ${fileName}: ${err.why}. ${err.todo}`;
}

/** Wrap anything thrown by an engine into a typed error, keeping the raw text for "Details". */
export function toRefractError(e: unknown): RefractError {
  if (e instanceof RefractError) return e;
  const raw = e instanceof Error ? `${e.name}: ${e.message}` : String(e);
  return new RefractError('Internal', 'something went wrong inside the converter', 'Try again. If it keeps happening, please report it.', raw);
}
