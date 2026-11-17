import type { ErrorCode, Inspect, Item, Options } from '@refract/core';

export type WorkerRequest =
  | { type: 'run'; op: string; items: Item[]; opts: Options }
  | { type: 'inspect'; items: Item[] }
  | { type: 'unlock'; item: Item; password: string };

export type WorkerResponse =
  | { type: 'heartbeat' }
  | { type: 'timing'; key: string; ms: number }
  | { type: 'done'; items?: Item[]; info?: Inspect[]; wrongPassword?: boolean }
  | { type: 'error'; code: ErrorCode; why: string; todo: string; details?: string };

/** What a codec module gets from the worker shell. */
export type Ctx = {
  heartbeat: () => void;
  timing: (key: string, ms: number) => void;
  /** Console lines captured since the job started (some WASM builds only log errors). */
  logs: string[];
};
