/// <reference lib="webworker" />
import { FORMATS, RefractError, type Inspect, type Item } from '@refract/core';
import type { Ctx, WorkerRequest, WorkerResponse } from './protocol.ts';

declare const self: DedicatedWorkerGlobalScope;

const post = (m: WorkerResponse, transfer: Transferable[] = []) => self.postMessage(m, transfer);

// Some WASM builds log failures to console instead of throwing; keep them for the "Details" toggle.
const logs: string[] = [];
for (const level of ['log', 'info', 'warn', 'error'] as const) {
  const original = console[level].bind(console);
  console[level] = (...args: unknown[]) => {
    logs.push(args.map(String).join(' '));
    original(...args);
  };
}

const ctx: Ctx = {
  heartbeat: () => post({ type: 'heartbeat' }),
  timing: (key, ms) => post({ type: 'timing', key, ms }),
  logs,
};

const codec = (kind: string) =>
  kind === 'image' ? import('./codecs/image.ts')
  : kind === 'pdf' ? import('./codecs/pdf.ts')
  : import('./codecs/audio.ts');

async function inspectAll(items: Item[]): Promise<Inspect[]> {
  const out: Inspect[] = [];
  for (const item of items) {
    out.push(await (await codec(FORMATS[item.format].kind)).inspect(item, ctx));
    ctx.heartbeat();
  }
  return out;
}

self.onmessage = async (e: MessageEvent<WorkerRequest>) => {
  const msg = e.data;
  // Async work (fetching and compiling WASM) can still beat; long synchronous calls can't, which the
  // watchdog's silence window allows for.
  const beat = setInterval(ctx.heartbeat, 1000);
  ctx.heartbeat();
  try {
    if (msg.type === 'run') {
      const items = await (await codec(msg.op.split('.')[0]!)).run(msg.op, msg.items, msg.opts, ctx);
      post({ type: 'done', items }, items.map((i) => i.bytes.buffer));
    } else if (msg.type === 'inspect') {
      post({ type: 'done', info: await inspectAll(msg.items) });
    } else {
      const result = await (await import('./codecs/pdf.ts')).unlock(msg.item, msg.password, ctx);
      if (result === 'wrong-password') post({ type: 'done', wrongPassword: true });
      else post({ type: 'done', items: [result] }, [result.bytes.buffer]);
    }
  } catch (err) {
    if (err instanceof RefractError) {
      post({ type: 'error', code: err.code, why: err.why, todo: err.todo, details: [err.details, ...logs].filter(Boolean).join('\n') || undefined });
    } else {
      post({
        type: 'error',
        code: 'Internal',
        why: 'something went wrong inside the converter',
        todo: 'Try again. If it keeps happening, please report it.',
        details: [err instanceof Error ? `${err.name}: ${err.message}` : String(err), ...logs].join('\n'),
      });
    }
  } finally {
    clearInterval(beat);
  }
};
