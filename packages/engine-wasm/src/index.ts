import { RefractError, type Edge, type Engine, type Format, type RunContext } from '@refract/core';
import type { WorkerRequest, WorkerResponse } from './protocol.ts';

export const ENGINE_VERSION = 'libheif-js@1.23.5+@jsquash/jpeg@1.6.0';
const MIME: Record<Format, string> = { heic: 'image/heic', jpeg: 'image/jpeg' };

type Done = Extract<WorkerResponse, { type: 'done' }>;

/** Run one request in a fresh worker. Cancel or timeout terminates the worker, which frees its memory. */
function inWorker(req: WorkerRequest, ctx: RunContext, transfer: Transferable[] = []): Promise<Done> {
  return new Promise((resolve, reject) => {
    const tSpawn = performance.now();
    let first = true;
    const worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
    const stop = () => worker.terminate();
    ctx.signal.addEventListener('abort', () => {
      stop();
      reject(new RefractError('Internal', 'the job was stopped', ''));
    });
    worker.onmessage = (e: MessageEvent<WorkerResponse>) => {
      const m = e.data;
      ctx.heartbeat();
      if (first && req.type === 'convert') ctx.report('workerStartMs', performance.now() - tSpawn);
      first = false;
      if (m.type === 'timing') ctx.report(m.key, m.ms);
      else if (m.type === 'done') { stop(); resolve(m); }
      else if (m.type === 'error') { stop(); reject(new RefractError(m.code, m.why, m.todo, m.details)); }
    };
    worker.onerror = (e) => {
      stop();
      reject(new RefractError('Internal', 'the converter crashed', 'Try again. If it keeps happening, please report it.', e.message));
    };
    worker.postMessage(req, transfer);
  });
}

const heicToJpeg: Edge = {
  from: 'heic',
  to: 'jpeg',
  engine: 'wasm',
  engineVersion: ENGINE_VERSION,
  options: [{ key: 'quality', label: 'Quality', type: 'number', min: 10, max: 100, default: 90 }],
  lossy: true,
  memFactor: 3, // RGBA in the libheif heap + JS copy + mozjpeg heap copy
  run: async (input, opts, ctx) => {
    // Transfer the input so the page doesn't hold a second copy while the worker decodes.
    const done = await inWorker({ type: 'convert', bytes: input, quality: opts.quality ?? 90 }, ctx, [input.buffer]);
    if (!done.bytes) throw new RefractError('Internal', 'the converter returned nothing', 'Try again.');
    return done.bytes;
  },
};

export const wasmEngine: Engine = {
  kind: 'wasm',
  edges: [heicToJpeg],
  decode: async (bytes, format, ctx) => {
    const done = await inWorker({ type: 'inspect', bytes, mime: MIME[format] }, ctx);
    return { width: done.width ?? 0, height: done.height ?? 0 };
  },
};
