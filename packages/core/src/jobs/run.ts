import { detect, extensionMismatch, type Format } from '../detect/detect.ts';
import { probeHeif, type HeifProbe } from '../detect/heif.ts';
import { RefractError, toRefractError } from '../errors.ts';
import { defaultOptions, planRoute, type CancelSignal, type Edge, type Options, type RunContext } from '../graph/graph.ts';
import { checkLimits, checkMemory, estimateImageMemory, limitsFor, type Platform } from '../presets/limits.ts';
import { CONVERTED, outputName } from '../presets/naming.ts';
import { verifyOutput } from '../verify/verify.ts';
import { canMove, type JobState } from './state.ts';

/** Implemented by engine-wasm (and later engine-native). core never imports an engine. */
export interface Engine {
  kind: Edge['engine'];
  edges: Edge[];
  /** Decode an output again with a real decoder and report its size (used by Verify). */
  decode: (bytes: Uint8Array, format: Format, ctx: RunContext) => Promise<{ width: number; height: number }>;
}

/** Injected so core needs no DOM or timer globals; tests pass a fake. */
export type Clock = { now(): number; every(ms: number, fn: () => void): () => void };

export type Step = {
  id: string;
  op: string;
  dependsOn: string[];
  status: 'waiting' | 'processing' | 'finished' | 'error';
  error?: RefractError;
};

export type Job = {
  id: string;
  inputName: string;
  outputName: string;
  state: JobState;
  inputSize: number;
  outputSize?: number;
  output?: Uint8Array;
  notice?: string;
  error?: RefractError;
  steps: Step[];
  /** Engine, pinned version and options of every step, so a result can be reproduced. */
  record?: { engine: string; engineVersion: string; options: Options }[];
  timings: Record<string, number>;
};

export type RunDeps = {
  engine: Engine;
  platform: Platform;
  op: string;
  target: Format;
  clock: Clock;
  /** True while the clock must stop, e.g. the tab is hidden and its workers are throttled. */
  isPaused: () => boolean;
  signal: CancelSignal;
  /** Concurrency slot; resolves with a release function. */
  acquire: () => Promise<() => void>;
  onUpdate: (job: Job) => void;
};

const EXT: Record<Format, string> = { heic: 'heic', jpeg: 'jpg' };
const LABEL: Record<Format, string> = { heic: 'HEIC', jpeg: 'JPEG' };
const PROBE: Partial<Record<Format, (b: Uint8Array) => HeifProbe>> = { heic: probeHeif };

class Canceled extends Error {}

export function createJob(id: string, inputName: string, inputSize: number, target: Format, taken: Set<string>): Job {
  return {
    id,
    inputName,
    outputName: outputName(CONVERTED, inputName, EXT[target], taken),
    state: 'Queued',
    inputSize,
    steps: [],
    timings: {},
  };
}

/** A limiter for the job queue (web: 2 at a time). */
export function createLimiter(n: number): () => Promise<() => void> {
  let active = 0;
  const waiting: (() => void)[] = [];
  return () =>
    new Promise((resolve) => {
      const start = () => {
        active++;
        let released = false;
        resolve(() => {
          if (released) return;
          released = true;
          active--;
          waiting.shift()?.();
        });
      };
      if (active < n) start();
      else waiting.push(start);
    });
}

function untilCanceled<T>(p: Promise<T>, signal: CancelSignal): Promise<T> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) return reject(new Canceled());
    signal.addEventListener('abort', () => reject(new Canceled()));
    p.then(resolve, reject);
  });
}

function cancelController() {
  let aborted = false;
  const listeners: (() => void)[] = [];
  const signal: CancelSignal = {
    get aborted() {
      return aborted;
    },
    addEventListener: (_type, fn) => (aborted ? fn() : void listeners.push(fn)),
  };
  return { signal, abort: () => { if (!aborted) { aborted = true; listeners.splice(0).forEach((f) => f()); } } };
}

type Guard = { timeoutMs: number; silenceMs: number; timeoutWhy: string };

/**
 * Run one piece of engine work under the watchdog: a timeout, and a heartbeat that must not go
 * silent. Both clocks stop while `isPaused()` is true, so a throttled background tab isn't killed.
 */
function guarded<T>(work: (ctx: RunContext) => Promise<T>, g: Guard, deps: RunDeps, job: Job): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const ctl = cancelController();
    let active = 0;
    let lastBeat = 0;
    let last = deps.clock.now();
    let settled = false;
    const finish = (fn: () => void) => {
      if (settled) return;
      settled = true;
      stop();
      fn();
    };
    const fail = (e: unknown) => finish(() => { ctl.abort(); reject(e); });
    const timeout = (why: string) =>
      new RefractError('Timeout', why, 'Try again with the tab in front, or try a smaller file.');
    const stop = deps.clock.every(250, () => {
      const now = deps.clock.now();
      if (!deps.isPaused()) active += now - last;
      last = now;
      if (active - lastBeat > g.silenceMs) fail(timeout('the converter stopped responding'));
      else if (active > g.timeoutMs) fail(timeout(g.timeoutWhy));
    });
    deps.signal.addEventListener('abort', () => fail(new Canceled()));
    const ctx: RunContext = {
      signal: ctl.signal,
      heartbeat: () => { lastBeat = active; },
      report: (key, ms) => { job.timings[key] = Math.round(ms); },
    };
    work(ctx).then((v) => finish(() => resolve(v)), (e) => fail(toRefractError(e)));
  });
}

/** Move one job through Queued → Probing → Running → Verifying → Saved (or Rejected/Failed/Canceled). */
/** `read` is only called after the size checks pass, so an oversized file is never loaded into memory. */
export async function runJob(job: Job, read: () => Promise<Uint8Array>, deps: RunDeps): Promise<Job> {
  const { clock, engine, signal } = deps;
  const set = (s: JobState) => {
    if (!canMove(job.state, s)) throw new Error(`Illegal job transition ${job.state} → ${s}`);
    job.state = s;
    deps.onUpdate(job);
  };
  const checkCanceled = () => { if (signal.aborted) throw new Canceled(); };
  const t0 = clock.now();
  let release: (() => void) | undefined;

  try {
    const slot = deps.acquire();
    try {
      release = await untilCanceled(slot, signal);
    } catch (e) {
      void slot.then((r) => r()); // canceled while queued: give the slot back once it arrives
      throw e;
    }

    // Probe: everything here is cheap and runs before any engine work.
    set('Probing');
    const limits = limitsFor(deps.op, deps.platform);
    checkLimits(limits, { bytes: job.inputSize });
    const input = job.inputSize === 0 ? new Uint8Array() : await untilCanceled(read(), signal);
    if (input.length === 0) {
      throw new RefractError('Corrupt', 'the file is empty (0 bytes)', 'Pick the original file again.');
    }
    const format = detect(input);
    if (format === 'unknown') {
      throw new RefractError('Unsupported', "Refract doesn't recognise this file type", 'Phase 1 converts HEIC photos only.');
    }
    job.notice = extensionMismatch(job.inputName, format) ?? undefined;
    const route = planRoute(engine.edges, format, deps.target, [engine.kind]);
    const probe = PROBE[format];
    if (!route || !probe) {
      throw new RefractError('Unsupported', `Refract can't convert ${LABEL[format]} to ${LABEL[deps.target]} yet`, 'Pick a HEIC photo.');
    }
    const { width, height } = probe(input);
    checkLimits(limits, { bytes: input.length, pixels: width * height });
    checkMemory(Math.max(...route.map((e) => estimateImageMemory(width, height, e.memFactor))), deps.platform);
    checkCanceled();
    job.timings.probeMs = clock.now() - t0;

    // Run: one step per edge; a failure marks later steps UpstreamFailed and keeps the root error.
    set('Running');
    job.steps = route.map((e, i) => ({
      id: `convert-${i + 1}`,
      op: `${e.from}→${e.to}`,
      dependsOn: i ? [`convert-${i}`] : [],
      status: 'waiting',
    }));
    job.record = route.map((e) => ({ engine: e.engine, engineVersion: e.engineVersion, options: defaultOptions(e) }));
    const mp = (width * height) / 1e6;
    const runGuard: Guard = {
      timeoutMs: 30_000 + 3_000 * mp,
      silenceMs: 10_000 + 500 * mp, // a big synchronous WASM decode can't post heartbeats
      timeoutWhy: `it took longer than ${Math.round((30_000 + 3_000 * mp) / 1000)} seconds`,
    };
    const tRun = clock.now();
    let data = input;
    for (const [i, edge] of route.entries()) {
      const step = job.steps[i]!;
      step.status = 'processing';
      deps.onUpdate(job);
      try {
        data = await guarded((ctx) => edge.run(data, job.record![i]!.options, ctx), runGuard, deps, job);
        step.status = 'finished';
      } catch (e) {
        if (e instanceof Canceled) throw e;
        step.status = 'error';
        step.error = toRefractError(e);
        for (const later of job.steps.slice(i + 1)) {
          later.status = 'error';
          later.error = new RefractError('UpstreamFailed', `an earlier step (${step.op}) failed`, '');
        }
        throw step.error;
      }
    }
    job.timings.runMs = clock.now() - tRun;

    // Verify: decode the output again; only a file that passes reaches Saved.
    set('Verifying');
    const tVerify = clock.now();
    const verifyGuard: Guard = { timeoutMs: 15_000 + 1_000 * mp, silenceMs: runGuard.silenceMs, timeoutWhy: 'finishing took too long' };
    const out = data;
    await guarded((ctx) => verifyOutput(out, { format: deps.target, width, height }, (b, f) => engine.decode(b, f, ctx)), verifyGuard, deps, job);
    job.timings.verifyMs = clock.now() - tVerify;

    job.output = out;
    job.outputSize = out.length;
    job.timings.totalMs = clock.now() - t0;
    set('Saved');
  } catch (e) {
    if (e instanceof Canceled || signal.aborted) {
      job.error = undefined;
      set('Canceled');
    } else {
      job.error = toRefractError(e);
      set(job.state === 'Probing' ? 'Rejected' : 'Failed');
    }
  } finally {
    release?.();
  }
  return job;
}
