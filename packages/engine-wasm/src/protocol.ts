import type { ErrorCode } from '@refract/core';

export type WorkerRequest =
  | { type: 'convert'; bytes: Uint8Array; quality: number }
  | { type: 'inspect'; bytes: Uint8Array; mime: string };

export type WorkerResponse =
  | { type: 'heartbeat' }
  | { type: 'timing'; key: string; ms: number }
  | { type: 'done'; bytes?: Uint8Array; width?: number; height?: number }
  | { type: 'error'; code: ErrorCode; why: string; todo: string; details?: string };
