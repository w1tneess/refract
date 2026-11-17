import { detect, extensionMismatch, formatFromName, FORMATS, type Format } from '../detect/detect.ts';
import { probeImage } from '../detect/image.ts';
import type { Engine } from '../engine.ts';
import { RefractError, toRefractError } from '../errors.ts';
import { resolveOptions, type CancelSignal, type Item, type Options, type RunContext } from '../graph/graph.ts';
import { checkLimits, checkMemory, limitsFor, type Platform } from '../presets/limits.ts';
import { outputName } from '../presets/naming.ts';
import { TOOLS, type Probed, type ToolId } from '../tools/tools.ts';
import { verifyOutputs } from '../verify/verify.ts';
import { canMove, type JobState } from './state.ts';

/** Injected so core needs no DOM or timer globals; tests pass a fake. */
export type Clock = { now(): number; every(ms: number, fn: () => void): () => void };

export type Step = {
  id: string;
  op: string;
  dependsOn: string[];
  status: 'waiting' | 'processing' | 'finished' | 'error';
  error?: RefractError;
};

export type JobInput = { name: string; size: number; read: () => Promise<Uint8Array> };
export type Output = { name: string; bytes: Uint8Array; format: Format };

export type Job = {
  id: string;
  tool: ToolId;
  options: Options;
  /** First input's name; the card's title. */
  inputName: string;
  inputNames: string[];
  /** The file the user saves: a single output, or the ZIP when `multi`. */
  outputName: string;
  multi: boolean;
  state: JobState;
  inputSize: number;
  outputSize?: number;
  outputs?: Output[];
  notice?: string;
  error?: RefractError;
  /** Set while state is NeedsPassword. */
  password?: { file: string; wrong: boolean };
  steps: Step[];
  /** Engine, pinned version and options of every step, so a result can be reproduced. */
  record?: { op: string; engineVersion?: string; options: Options }[];
  timings: Record<string, number>;
};

export type RunDeps = {
  engine: Engine;
  engineVersion: string;
  platform: Platform;
  clock: Clock;
  /** True while the clock must stop, e.g. the tab is hidden and its workers are throttled. */
  isPaused: () => boolean;
  signal: CancelSignal;
  /** Concurrency slot; resolves with a release function. */
  acquire: () => Promise<() => void>;
  /** Asked when a PDF is locked. Resolve the password, or null to skip the file. */
  askPassword: (job: Job, file: string, wrong: boolean) => Promise<string | null>;
  onUpdate: (job: Job) => void;
};

class Canceled extends Error {
  constructor(readonly notice?: string) { super('canceled'); }
}

export function createJob(id: string, toolId: ToolId, options: Options, inputs: { name: string; size: number }[], taken: Set<string>): Job {
  const tool = TOOLS[toolId];
  const opts = resolveOptions(tool.options, options);
  const first = inputs[0]?.name ?? 'file';
  const out = tool.output(opts, formatFromName(first));
  return {
    id,
    tool: toolId,
    options: opts,
    inputName: first,
    inputNames: inputs.map((i) => i.name),
    outputName: outputName(out.template, first, out.ext, taken),
    multi: out.multi,
    state: 'Queued',
    inputSize: inputs.reduce((n, i) => n + i.size, 0),
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
      report: (key, ms) => { job.timings[key] = Math.round((job.timings[key] ?? 0) + ms); },
    };
    work(ctx).then((v) => finish(() => resolve(v)), (e) => fail(e instanceof Canceled ? e : toRefractError(e)));
  });
}

/** Watchdog budgets from the amount of work: megapixels, pages and seconds of audio. */
function budgets(inputs: Probed[]) {
  const mp = inputs.reduce((n, p) => n + (p.image ? (p.image.width * p.image.height) / 1e6 : 0), 0);
  const pages = inputs.reduce((n, p) => n + (p.pages ?? 0), 0);
  const secs = inputs.reduce((n, p) => n + (p.durationS ?? 0), 0);
  const timeoutMs = 30_000 + 3_000 * mp + 1_000 * pages + 500 * secs;
  // A big synchronous WASM call (decode, encode, ffmpeg) can't post heartbeats while it runs.
  const silenceMs = 15_000 + 1_000 * mp + 200 * pages + 200 * secs;
  return {
    run: { timeoutMs, silenceMs, timeoutWhy: `it took longer than ${Math.round(timeoutMs / 1000)} seconds` },
    finish: { timeoutMs: 20_000 + timeoutMs / 3, silenceMs, timeoutWhy: 'finishing took too long' },
    probe: { timeoutMs: 60_000, silenceMs: 30_000, timeoutWhy: 'opening the file took too long' },
  } satisfies Record<string, Guard>;
}

/** Move one job through Queued → Probing → (NeedsPassword) → Running → Verifying → Saved, or Rejected/Failed/Canceled. */
export async function runJob(job: Job, inputs: JobInput[], deps: RunDeps): Promise<Job> {
  const { clock, engine, signal } = deps;
  const tool = TOOLS[job.tool];
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

    // ── Probe: cheap checks, all before any conversion work.
    set('Probing');
    const limits = limitsFor(tool.limits, deps.platform);
    checkLimits(limits, { bytes: 0, files: inputs.length });
    if (inputs.length < (tool.minInputs ?? 1)) {
      throw new RefractError('Unsupported', `${tool.label} needs at least ${tool.minInputs} files`, 'Add more files to this job.');
    }
    const notices: string[] = [];
    const probed: Probed[] = [];
    for (const input of inputs) {
      checkLimits(limits, { bytes: input.size });
      const bytes = input.size === 0 ? new Uint8Array() : await untilCanceled(input.read(), signal);
      if (bytes.length === 0) {
        throw new RefractError('Corrupt', `${inputs.length > 1 ? `${input.name} is empty` : 'the file is empty'} (0 bytes)`, 'Pick the original file again.');
      }
      const format = detect(bytes);
      if (format === 'unknown') {
        throw new RefractError('Unsupported', "Refract doesn't recognise this file type", `${tool.label} takes ${tool.accepts} files.`);
      }
      if (FORMATS[format].kind !== tool.accepts) {
        throw new RefractError('Unsupported', `${input.name} is a ${FORMATS[format].label} file`, `${tool.label} takes ${tool.accepts} files.`);
      }
      const mismatch = extensionMismatch(input.name, format);
      if (mismatch) notices.push(mismatch);
      const p: Probed = { name: input.name, item: { bytes, format } };
      if (FORMATS[format].kind === 'image') {
        p.image = probeImage(format, bytes);
        checkLimits(limits, { bytes: bytes.length, pixels: p.image.width * p.image.height });
      }
      probed.push(p);
    }

    // PDFs and audio need a real decoder to count pages or measure length.
    const toInspect = probed.filter((p) => !p.image);
    if (toInspect.length) {
      const info = await guarded((ctx) => engine.inspect(toInspect.map((p) => p.item), ctx), budgets([]).probe, deps, job);
      for (const [i, p] of toInspect.entries()) {
        const got = info[i]!;
        if (got.format === 'unknown') {
          throw new RefractError('Corrupt', `${inputs.length > 1 ? `${p.name} is damaged` : 'the file is damaged'} and can't be opened`, 'Try re-downloading it.');
        }
        if (got.encrypted) await unlock(p, job, deps, set);
        p.pages = got.pages;
        p.durationS = got.durationS;
        if (p.pages !== undefined) checkLimits(limits, { bytes: p.item.bytes.length, pages: p.pages });
      }
    }
    checkMemory(tool.memory(probed, job.options), deps.platform);
    const plan = tool.plan(probed, job.options, engine);
    checkCanceled();
    job.notice = notices.join(' ') || undefined;
    job.timings.probeMs = clock.now() - t0;

    // ── Run: one step per planned op; a failure marks later steps UpstreamFailed and keeps the root error.
    set('Running');
    const b = budgets(probed);
    job.steps = plan.steps.map((s, i) => ({
      id: `step-${i + 1}`,
      op: s.op,
      dependsOn: i ? [`step-${i}`] : [],
      status: 'waiting',
    }));
    job.record = plan.steps.map((s) => ({ op: s.op, engineVersion: s.engineVersion ?? deps.engineVersion, options: s.opts }));
    const tRun = clock.now();
    let data: Item[] = probed.map((p) => p.item);
    for (const [i, s] of plan.steps.entries()) {
      const step = job.steps[i]!;
      step.status = 'processing';
      deps.onUpdate(job);
      try {
        const current = data;
        data = await guarded((ctx) => s.run(current, ctx), b.run, deps, job);
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

    // ── Verify: reopen every output; only files that pass reach Saved. Has its own, shorter timeout.
    set('Verifying');
    const tVerify = clock.now();
    const outs = data;
    await guarded((ctx) => verifyOutputs(outs, plan.expect, (items) => engine.inspect(items, ctx)), b.finish, deps, job);
    job.timings.verifyMs = clock.now() - tVerify;

    const names = job.multi ? plan.entryNames ?? outs.map((_, i) => `${i + 1}`) : [job.outputName];
    job.outputs = outs.map((o, i) => ({ name: names[i]!, bytes: o.bytes, format: o.format }));
    job.outputSize = outs.reduce((n, o) => n + o.bytes.length, 0);
    job.notice = [job.notice, plan.notice?.(outs)].filter(Boolean).join(' ') || undefined;
    job.timings.totalMs = clock.now() - t0;
    set('Saved');
  } catch (e) {
    job.password = undefined;
    if (e instanceof Canceled || signal.aborted) {
      job.error = undefined;
      if (e instanceof Canceled && e.notice) job.notice = e.notice;
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

/** Ask for a password until one opens the PDF, or the user skips it. An empty password is tried first (owner-only locks). */
async function unlock(p: Probed, job: Job, deps: RunDeps, set: (s: JobState) => void): Promise<void> {
  const tryOne = (pw: string) => guarded((ctx) => deps.engine.unlock(p.item, pw, ctx), budgets([]).probe, deps, job);
  let result = await tryOne('');
  let wrong = false;
  while (result === 'wrong-password') {
    job.password = { file: p.name, wrong };
    set('NeedsPassword');
    const pw = await untilCanceled(deps.askPassword(job, p.name, wrong), deps.signal);
    job.password = undefined;
    if (pw === null) throw new Canceled(`Skipped ${p.name}: it is password-protected.`);
    set('Probing');
    result = await tryOne(pw);
    wrong = true;
  }
  p.item = result;
}
