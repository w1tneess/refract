import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  batchStatus, canMove, createJob, createLimiter, planRoute, runJob, verifyOutput, readJpegInfo,
  type Edge, type Engine, type RunContext, type RunDeps,
} from '../src/index.ts';
import { fakeClock, fakeJpeg, flush } from './helpers.ts';

const heic = new Uint8Array(readFileSync(new URL('../../../fixtures/plain.heic', import.meta.url)));
const rotated = new Uint8Array(readFileSync(new URL('../../../fixtures/rotated.heic', import.meta.url)));

type Run = Edge['run'];
const edge = (run: Run, over: Partial<Edge> = {}): Edge => ({
  from: 'heic', to: 'jpeg', engine: 'wasm', engineVersion: 'test@1', lossy: true, memFactor: 2.5,
  options: [{ key: 'quality', label: 'Quality', type: 'number', min: 10, max: 100, default: 90 }],
  run, ...over,
});
const ok = (w = 400, h = 300): Run => async (_i, _o, ctx) => { ctx.heartbeat(); return fakeJpeg(w, h); };

function setup(edges: Edge[], decode: Engine['decode'] = async (b) => readJpegInfo(b)) {
  const { clock, advance } = fakeClock();
  let aborted = false;
  const listeners: (() => void)[] = [];
  let paused = false;
  const states: string[] = [];
  const deps: RunDeps = {
    engine: { kind: 'wasm', edges, decode },
    platform: 'web', op: 'image.convert', target: 'jpeg', clock,
    isPaused: () => paused,
    signal: { get aborted() { return aborted; }, addEventListener: (_t, f) => void listeners.push(f) },
    acquire: createLimiter(2),
    onUpdate: (j) => { if (states.at(-1) !== j.state) states.push(j.state); },
  };
  const cancel = () => { aborted = true; listeners.forEach((f) => f()); };
  const job = () => createJob('1', 'IMG.HEIC', heic.length, 'jpeg', new Set());
  return { deps, advance, cancel, states, job, setPaused: (p: boolean) => (paused = p) };
}

describe('state machine', () => {
  it('only allows the documented moves', () => {
    expect(canMove('Queued', 'Probing')).toBe(true);
    expect(canMove('Queued', 'Running')).toBe(false);
    expect(canMove('Saved', 'Canceled')).toBe(false);
    for (const s of ['Queued', 'Probing', 'Running', 'Verifying'] as const) expect(canMove(s, 'Canceled')).toBe(true);
  });
  it('batch status: processing > waiting > error > finished; Canceled is not an error', () => {
    expect(batchStatus(['Failed', 'Queued', 'Running'])).toBe('processing');
    expect(batchStatus(['Failed', 'Queued'])).toBe('waiting');
    expect(batchStatus(['Saved', 'Rejected'])).toBe('error');
    expect(batchStatus(['Saved', 'Canceled'])).toBe('finished');
  });
});

describe('runJob', () => {
  it('goes Queued → Probing → Running → Verifying → Saved and records engine, version and options', async () => {
    const s = setup([edge(ok())]);
    const j = await runJob(s.job(), heic, s.deps);
    expect(s.states).toEqual(['Probing', 'Running', 'Verifying', 'Saved']);
    expect(j.outputName).toBe('IMG (converted).jpg');
    expect(j.record).toEqual([{ engine: 'wasm', engineVersion: 'test@1', options: { quality: 90 } }]);
    expect(j.outputSize).toBeGreaterThan(0);
  });

  it('expects rotated dimensions for a rotated HEIC', async () => {
    const s = setup([edge(ok(300, 400))]);
    expect((await runJob(s.job(), rotated, s.deps)).state).toBe('Saved');
    const s2 = setup([edge(ok(400, 300))]); // forgot to rotate
    const j = await runJob(s2.job(), rotated, s2.deps);
    expect(j.state).toBe('Failed');
    expect(j.error?.why).toMatch(/400×300, expected 300×400/);
  });

  it('cancel while running ends Canceled, not Failed, and aborts the engine', async () => {
    let engineAborted = false;
    const s = setup([edge((_i, _o, ctx) => new Promise(() => ctx.signal.addEventListener('abort', () => (engineAborted = true))))]);
    const p = runJob(s.job(), heic, s.deps);
    await flush();
    expect(s.states.at(-1)).toBe('Running');
    s.cancel();
    const j = await p;
    expect(j.state).toBe('Canceled');
    expect(j.error).toBeUndefined();
    expect(engineAborted).toBe(true);
  });

  it('cancel while queued ends Canceled and frees the slot', async () => {
    const s = setup([edge(ok())]);
    const hold = await s.deps.acquire();
    const hold2 = await s.deps.acquire();
    const p = runJob(s.job(), heic, s.deps);
    await flush();
    s.cancel();
    expect((await p).state).toBe('Canceled');
    expect(s.states).toEqual(['Canceled']);
    hold(); hold2();
  });

  it('empty file → Rejected Corrupt; unknown bytes → Rejected Unsupported', async () => {
    const s = setup([edge(ok())]);
    const a = await runJob(s.job(), new Uint8Array(), s.deps);
    expect([a.state, a.error?.code]).toEqual(['Rejected', 'Corrupt']);
    const b = await runJob(s.job(), new Uint8Array(64), s.deps);
    expect([b.state, b.error?.code]).toEqual(['Rejected', 'Unsupported']);
  });

  it('over the memory budget → Rejected TooLarge before the engine runs', async () => {
    let ran = false;
    const s = setup([edge(async () => { ran = true; return fakeJpeg(1, 1); }, { memFactor: 10_000 })]);
    const j = await runJob(s.job(), heic, s.deps);
    expect([j.state, j.error?.code, ran]).toEqual(['Rejected', 'TooLarge', false]);
    expect(j.error?.why).toMatch(/MB of memory/);
  });

  it('engine error → Failed with a typed error and the raw text in details', async () => {
    const s = setup([edge(async () => { throw new Error('libheif: bad stream'); })]);
    const j = await runJob(s.job(), heic, s.deps);
    expect([j.state, j.error?.code]).toEqual(['Failed', 'Internal']);
    expect(j.error?.details).toMatch(/bad stream/);
  });

  it('planRoute reads the edge table: least quality loss, then fewest hops; lossy middles are skipped', () => {
    const png = 'png' as never;
    const direct = edge(ok());
    const toPng = edge(ok(), { to: png, lossy: false });
    const pngToJpeg = edge(ok(), { from: png });
    const lossyMiddle = edge(ok(), { to: png });
    expect(planRoute([toPng, pngToJpeg, direct], 'heic', 'jpeg', ['wasm'])).toEqual([direct]);
    expect(planRoute([toPng, pngToJpeg], 'heic', 'jpeg', ['wasm'])).toEqual([toPng, pngToJpeg]);
    expect(planRoute([lossyMiddle, pngToJpeg], 'heic', 'jpeg', ['wasm'])).toBeNull();
    expect(planRoute([direct], 'heic', 'jpeg', ['native'])).toBeNull();
  });

  it('a two-hop route reports the first real error, and the later step gets UpstreamFailed', async () => {
    const { RefractError } = await import('../src/index.ts');
    const png = 'png' as never;
    const hop1 = edge(async () => { throw new RefractError('Corrupt', "couldn't decode HEIC", 'Try again.'); }, { to: png, lossy: false });
    const hop2 = edge(ok(), { from: png });
    const s = setup([hop1, hop2]);
    const j = await runJob(s.job(), heic, s.deps);
    expect(j.state).toBe('Failed');
    expect(j.error?.why).toBe("couldn't decode HEIC");
    expect(j.steps.map((x) => [x.id, x.dependsOn, x.status, x.error?.code])).toEqual([
      ['convert-1', [], 'error', 'Corrupt'],
      ['convert-2', ['convert-1'], 'error', 'UpstreamFailed'],
    ]);
  });

  it('a hung engine times out on heartbeat silence → Failed Timeout', async () => {
    const s = setup([edge(() => new Promise(() => {}))]);
    const p = runJob(s.job(), heic, s.deps);
    await flush();
    await s.advance(12_000);
    const j = await p;
    expect([j.state, j.error?.code]).toEqual(['Failed', 'Timeout']);
    expect(j.error?.why).toMatch(/stopped responding/);
  });

  it('the clock stops while the tab is hidden, so a throttled job is not killed', async () => {
    let finish!: (b: Uint8Array) => void;
    const s = setup([edge((_i, _o, ctx: RunContext) => { ctx.heartbeat(); return new Promise((r) => (finish = r)); })]);
    const p = runJob(s.job(), heic, s.deps);
    await flush();
    s.setPaused(true);
    await s.advance(120_000);
    expect(s.states.at(-1)).toBe('Running');
    s.setPaused(false);
    finish(fakeJpeg(400, 300));
    expect((await p).state).toBe('Saved');
  });

  it('verify rejects outputs that are the wrong type, truncated or still carry EXIF', async () => {
    const dims = async (b: Uint8Array) => readJpegInfo(b);
    const exp = { format: 'jpeg' as const, width: 400, height: 300 };
    await expect(verifyOutput(new Uint8Array(), exp, dims)).rejects.toThrow(/empty/);
    await expect(verifyOutput(heic, exp, dims)).rejects.toThrow(/heic, not jpeg/);
    await expect(verifyOutput(fakeJpeg(400, 300, { truncated: true }), exp, dims)).rejects.toThrow(/ends early/);
    await expect(verifyOutput(fakeJpeg(400, 300, { exif: true }), exp, dims)).rejects.toThrow(/EXIF/);
    await expect(verifyOutput(fakeJpeg(400, 300), exp, dims)).resolves.toBeUndefined();
  });
});
