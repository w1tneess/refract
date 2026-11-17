import { RefractError, formatsOfKind, type Edge, type Engine, type Format, type Item, type RunContext } from '@refract/core';
import type { WorkerRequest, WorkerResponse } from './protocol.ts';

export const ENGINE_VERSION = 'libheif-js@1.23.5+@jsquash/jpeg@1.6.0+@jsquash/png@3.1.1+@jsquash/webp@1.5.0+@jsquash/resize@2.1.1+pdf-lib@1.17.1+pdfjs-dist@6.5.438+@neslinesli93/qpdf-wasm@0.3.0+@ffmpeg/ffmpeg@0.12.15+@ffmpeg/core@0.12.10';

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
      if (first && req.type === 'run') ctx.report('workerStartMs', performance.now() - tSpawn);
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

// Copies, not transfers: the job keeps its inputs for Retry and for verifying against them.
const copy = (items: Item[]) => items.map((i) => ({ bytes: i.bytes.slice(), format: i.format }));

async function run(op: string, items: Item[], opts: Record<string, number | string>, ctx: RunContext): Promise<Item[]> {
  const input = copy(items);
  const done = await inWorker({ type: 'run', op, items: input, opts }, ctx, input.map((i) => i.bytes.buffer));
  if (!done.items?.length) throw new RefractError('Internal', 'the converter returned nothing', 'Try again.');
  return done.items;
}

function edges(): Edge[] {
  const out: Edge[] = [];
  const imageOut: Format[] = ['jpeg', 'png', 'webp'];
  for (const from of formatsOfKind('image')) {
    for (const to of imageOut) {
      if (from === to) continue;
      out.push({
        from, to, engine: 'wasm', engineVersion: ENGINE_VERSION, options: [],
        lossy: to !== 'png', memFactor: 3, // RGBA in the decoder heap + JS copy + encoder heap copy
        run: (items, opts, ctx) => run('image.convert', items, { ...opts, to }, ctx),
      });
    }
  }
  for (const from of formatsOfKind('audio')) {
    for (const to of formatsOfKind('audio')) {
      if (from === to) continue;
      out.push({
        from, to, engine: 'wasm', engineVersion: ENGINE_VERSION, options: [],
        lossy: to !== 'wav', memFactor: 4,
        run: (items, opts, ctx) => run('audio.convert', items, { ...opts, to }, ctx),
      });
    }
  }
  return out;
}

export const wasmEngine: Engine = {
  kind: 'wasm',
  edges: edges(),
  run,
  inspect: async (items, ctx) => (await inWorker({ type: 'inspect', items: copy(items) }, ctx)).info ?? [],
  unlock: async (item, password, ctx) => {
    const done = await inWorker({ type: 'unlock', item: copy([item])[0]!, password }, ctx);
    return done.wrongPassword ? 'wrong-password' : done.items![0]!;
  },
};
