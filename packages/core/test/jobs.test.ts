import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  batchStatus, canMove, createJob, createLimiter, detect, planRoute, readJpegInfo, RefractError, runJob, verifyOutputs,
  type Edge, type Engine, type Inspect, type Item, type JobInput, type Options, type RunDeps, type ToolId,
} from '../src/index.ts';
import { fakeClock, fakeJpeg, fakeMp3, fakePdf, flush } from './helpers.ts';

const heic = new Uint8Array(readFileSync(new URL('../../../fixtures/plain.heic', import.meta.url)));
const rotated = new Uint8Array(readFileSync(new URL('../../../fixtures/rotated.heic', import.meta.url)));

type Run = Edge['run'];
const edge = (run: Run, over: Partial<Edge> = {}): Edge => ({
  from: 'heic', to: 'jpeg', engine: 'wasm', engineVersion: 'test@1', lossy: true, memFactor: 3, options: [], run, ...over,
});
const jpegOut = (w = 400, h = 300): Run => async (_i, _o, ctx) => { ctx.heartbeat(); return [{ bytes: fakeJpeg(w, h), format: 'jpeg' }]; };

/** Inspect fake: images report their SOF size; PDFs and audio report from a lookup by first bytes. */
const facts = new Map<string, Inspect>();
const key = (b: Uint8Array) => new TextDecoder().decode(b.subarray(0, 24));
async function inspect(items: Item[]): Promise<Inspect[]> {
  return items.map((i) => {
    if (i.format === 'jpeg') { const j = readJpegInfo(i.bytes); return { format: 'jpeg', width: j.width, height: j.height }; }
    return facts.get(key(i.bytes)) ?? { format: detect(i.bytes) };
  });
}

function setup(edges: Edge[], over: Partial<Engine> = {}, passwords: (string | null)[] = []) {
  const { clock, advance } = fakeClock();
  let aborted = false;
  const listeners: (() => void)[] = [];
  let paused = false;
  const states: string[] = [];
  const asked: boolean[] = [];
  const engine: Engine = {
    kind: 'wasm', edges, inspect: (items) => inspect(items),
    run: async () => { throw new Error('no op'); },
    unlock: async () => 'wrong-password',
    ...over,
  };
  const deps: RunDeps = {
    engine, engineVersion: 'test@1', platform: 'web', clock,
    isPaused: () => paused,
    signal: { get aborted() { return aborted; }, addEventListener: (_t, f) => void listeners.push(f) },
    acquire: createLimiter(2),
    askPassword: async (_j, _f, wrong) => { asked.push(wrong); return passwords.shift() ?? null; },
    onUpdate: (j) => { if (states.at(-1) !== j.state) states.push(j.state); },
  };
  const cancel = () => { aborted = true; listeners.forEach((f) => f()); };
  const job = (tool: ToolId = 'convert-image', opts: Options = {}, names = ['IMG.HEIC'], size = heic.length) =>
    createJob('1', tool, opts, names.map((name) => ({ name, size })), new Set());
  const inputs = (...files: [string, Uint8Array][]): JobInput[] =>
    files.map(([name, bytes]) => ({ name, size: bytes.length, read: async () => bytes }));
  return { deps, advance, cancel, states, job, inputs, asked, setPaused: (p: boolean) => (paused = p) };
}

describe('state machine', () => {
  it('only allows the documented moves', () => {
    expect(canMove('Queued', 'Probing')).toBe(true);
    expect(canMove('Queued', 'Running')).toBe(false);
    expect(canMove('Probing', 'NeedsPassword')).toBe(true);
    expect(canMove('NeedsPassword', 'Probing')).toBe(true);
    expect(canMove('NeedsPassword', 'Running')).toBe(false);
    expect(canMove('Saved', 'Canceled')).toBe(false);
    for (const s of ['Queued', 'Probing', 'NeedsPassword', 'Running', 'Verifying'] as const) expect(canMove(s, 'Canceled')).toBe(true);
  });
  it('batch status: processing > waiting > error > finished; Canceled is not an error', () => {
    expect(batchStatus(['Failed', 'Queued', 'Running'])).toBe('processing');
    expect(batchStatus(['Failed', 'NeedsPassword'])).toBe('waiting');
    expect(batchStatus(['Saved', 'Rejected'])).toBe('error');
    expect(batchStatus(['Saved', 'Canceled'])).toBe('finished');
  });
});

describe('runJob: convert', () => {
  it('goes Queued → Probing → Running → Verifying → Saved and records the step, version and options', async () => {
    const s = setup([edge(jpegOut())]);
    const j = await runJob(s.job(), s.inputs(['IMG.HEIC', heic]), s.deps);
    expect(s.states).toEqual(['Probing', 'Running', 'Verifying', 'Saved']);
    expect(j.outputName).toBe('IMG (converted).jpg');
    expect(j.outputs?.map((o) => o.name)).toEqual(['IMG (converted).jpg']);
    expect(j.record).toEqual([{ op: 'heic→jpeg', engineVersion: 'test@1', options: { to: 'jpeg', quality: 90 } }]);
  });

  it('expects rotated dimensions for a rotated HEIC', async () => {
    const s = setup([edge(jpegOut(300, 400))]);
    expect((await runJob(s.job(), s.inputs(['r.heic', rotated]), s.deps)).state).toBe('Saved');
    const s2 = setup([edge(jpegOut(400, 300))]); // forgot to rotate
    const j = await runJob(s2.job(), s2.inputs(['r.heic', rotated]), s2.deps);
    expect(j.state).toBe('Failed');
    expect(j.error?.why).toMatch(/400×300, expected 300×400/);
  });

  it('cancel while running ends Canceled, not Failed, and aborts the engine', async () => {
    let engineAborted = false;
    const s = setup([edge((_i, _o, ctx) => new Promise(() => ctx.signal.addEventListener('abort', () => (engineAborted = true))))]);
    const p = runJob(s.job(), s.inputs(['IMG.HEIC', heic]), s.deps);
    await flush();
    expect(s.states.at(-1)).toBe('Running');
    s.cancel();
    const j = await p;
    expect([j.state, j.error, engineAborted]).toEqual(['Canceled', undefined, true]);
  });

  it('cancel while queued ends Canceled and frees the slot', async () => {
    const s = setup([edge(jpegOut())]);
    const hold = [await s.deps.acquire(), await s.deps.acquire()];
    const p = runJob(s.job(), s.inputs(['IMG.HEIC', heic]), s.deps);
    await flush();
    s.cancel();
    expect((await p).state).toBe('Canceled');
    expect(s.states).toEqual(['Canceled']);
    hold.forEach((r) => r());
  });

  it('empty → Rejected Corrupt; unknown → Unsupported; wrong kind for the tool → Unsupported', async () => {
    const s = setup([edge(jpegOut())]);
    const a = await runJob(s.job(), s.inputs(['e.heic', new Uint8Array()]), s.deps);
    expect([a.state, a.error?.code]).toEqual(['Rejected', 'Corrupt']);
    const b = await runJob(s.job(), s.inputs(['x.heic', new Uint8Array(64)]), s.deps);
    expect([b.state, b.error?.code]).toEqual(['Rejected', 'Unsupported']);
    const c = await runJob(s.job(), s.inputs(['song.mp3', fakeMp3()]), s.deps);
    expect([c.state, c.error?.why]).toEqual(['Rejected', 'song.mp3 is a MP3 file']);
  });

  it('converting to the same format is rejected with a pointer to the right tool', async () => {
    const s = setup([]);
    const j = await runJob(s.job('convert-image', { to: 'jpeg' }, ['a.jpg']), s.inputs(['a.jpg', fakeJpeg(10, 10)]), s.deps);
    expect([j.state, j.error?.why]).toEqual(['Rejected', 'it is already a JPG file']);
  });

  it('over maxMB → Rejected TooLarge without ever reading the file', async () => {
    const s = setup([edge(jpegOut())]);
    let read = false;
    const j = await runJob(s.job(), [{ name: 'big.heic', size: 500 * 1024 * 1024, read: async () => { read = true; return heic; } }], s.deps);
    expect([j.state, j.error?.code, read]).toEqual(['Rejected', 'TooLarge', false]);
  });

  it('over the memory budget → Rejected TooLarge before the engine runs', async () => {
    let ran = false;
    const s = setup([], { run: async () => { ran = true; return []; } });
    const big = fakeJpeg(10_000, 9_000); // 90 MP: under maxPixels, over the memory budget for compress
    const j = await runJob(s.job('compress-image', {}, ['a.jpg']), s.inputs(['a.jpg', big]), s.deps);
    expect([j.state, j.error?.code, ran]).toEqual(['Rejected', 'TooLarge', false]);
    expect(j.error?.why).toMatch(/MB of memory/);
  });

  it('engine error → Failed with a typed error and the raw text in details', async () => {
    const s = setup([edge(async () => { throw new Error('libheif: bad stream'); })]);
    const j = await runJob(s.job(), s.inputs(['IMG.HEIC', heic]), s.deps);
    expect([j.state, j.error?.code]).toEqual(['Failed', 'Internal']);
    expect(j.error?.details).toMatch(/bad stream/);
  });

  it('planRoute: least quality loss, then fewest hops; lossy middles skipped', () => {
    const direct = edge(jpegOut(), { to: 'webp' });
    const toPng = edge(jpegOut(), { to: 'png', lossy: false });
    const pngToWebp = edge(jpegOut(), { from: 'png', to: 'webp' });
    const lossyMiddle = edge(jpegOut(), { to: 'png' });
    expect(planRoute([toPng, pngToWebp, direct], 'heic', 'webp', ['wasm'])).toEqual([direct]);
    expect(planRoute([toPng, pngToWebp], 'heic', 'webp', ['wasm'])).toEqual([toPng, pngToWebp]);
    expect(planRoute([lossyMiddle, pngToWebp], 'heic', 'webp', ['wasm'])).toBeNull();
    expect(planRoute([direct], 'heic', 'webp', ['native'])).toBeNull();
  });

  it('a two-hop route reports the first real error, and the later step gets UpstreamFailed', async () => {
    const hop1 = edge(async () => { throw new RefractError('Corrupt', "couldn't decode HEIC", 'Try again.'); }, { to: 'png', lossy: false });
    const hop2 = edge(jpegOut(), { from: 'png', to: 'webp' });
    const s = setup([hop1, hop2]);
    const j = await runJob(s.job('convert-image', { to: 'webp' }), s.inputs(['IMG.HEIC', heic]), s.deps);
    expect(j.state).toBe('Failed');
    expect(j.error?.why).toBe("couldn't decode HEIC");
    expect(j.steps.map((x) => [x.id, x.dependsOn, x.status, x.error?.code])).toEqual([
      ['step-1', [], 'error', 'Corrupt'],
      ['step-2', ['step-1'], 'error', 'UpstreamFailed'],
    ]);
  });

  it('a hung engine times out on heartbeat silence → Failed Timeout', async () => {
    const s = setup([edge(() => new Promise(() => {}))]);
    const p = runJob(s.job(), s.inputs(['IMG.HEIC', heic]), s.deps);
    await flush();
    await s.advance(20_000);
    const j = await p;
    expect([j.state, j.error?.code]).toEqual(['Failed', 'Timeout']);
    expect(j.error?.why).toMatch(/stopped responding/);
  });

  it('the clock stops while the tab is hidden, so a throttled job is not killed', async () => {
    let finish!: (i: Item[]) => void;
    const s = setup([edge((_i, _o, ctx) => { ctx.heartbeat(); return new Promise((r) => (finish = r)); })]);
    const p = runJob(s.job(), s.inputs(['IMG.HEIC', heic]), s.deps);
    await flush();
    s.setPaused(true);
    await s.advance(120_000);
    expect(s.states.at(-1)).toBe('Running');
    s.setPaused(false);
    finish([{ bytes: fakeJpeg(400, 300), format: 'jpeg' }]);
    expect((await p).state).toBe('Saved');
  });
});

describe('runJob: PDF tools', () => {
  const pdfA = fakePdf('A');
  const pdfB = fakePdf('B');
  const locked = fakePdf('LOCKED');
  const unlocked = fakePdf('UNLOCKED');
  const onePage = fakePdf('ONE');
  facts.set(key(pdfA), { format: 'pdf', pages: 2 });
  facts.set(key(pdfB), { format: 'pdf', pages: 3 });
  facts.set(key(locked), { format: 'pdf', pages: 1, encrypted: true });
  facts.set(key(unlocked), { format: 'pdf', pages: 1 });
  facts.set(key(onePage), { format: 'pdf', pages: 1 });

  it('merge needs at least two files', async () => {
    const s = setup([]);
    const j = await runJob(s.job('merge-pdf', {}, ['a.pdf']), s.inputs(['a.pdf', pdfA]), s.deps);
    expect([j.state, j.error?.why]).toEqual(['Rejected', 'Merge PDFs needs at least 2 files']);
  });

  it('merge expects the sum of the pages', async () => {
    const merged = fakePdf('MERGED');
    facts.set(key(merged), { format: 'pdf', pages: 5 });
    const s = setup([], { run: async (op) => (op === 'pdf.merge' ? [{ bytes: merged, format: 'pdf' }] : []) });
    const j = await runJob(s.job('merge-pdf', {}, ['a.pdf', 'b.pdf']), s.inputs(['a.pdf', pdfA], ['b.pdf', pdfB]), s.deps);
    expect([j.state, j.outputName]).toEqual(['Saved', 'a_merged.pdf']);
  });

  it('split names every file inside one ZIP and checks each page count', async () => {
    const s = setup([], { run: async () => [onePage, onePage, onePage].map((bytes) => ({ bytes, format: 'pdf' as const })) });
    const j = await runJob(s.job('split-pdf', {}, ['doc.pdf']), s.inputs(['doc.pdf', pdfB]), s.deps);
    expect([j.state, j.multi, j.outputName]).toEqual(['Saved', true, 'doc_split.zip']);
    expect(j.outputs?.map((o) => o.name)).toEqual(['doc-p1.pdf', 'doc-p2.pdf', 'doc-p3.pdf']);
  });

  it('a wrong number of outputs fails Verify', async () => {
    const s = setup([], { run: async () => [{ bytes: onePage, format: 'pdf' as const }] });
    const j = await runJob(s.job('split-pdf', {}, ['doc.pdf']), s.inputs(['doc.pdf', pdfB]), s.deps);
    expect([j.state, j.error?.why]).toEqual(['Failed', "the converted file didn't pass the final check (1 files came out, expected 3)"]);
  });

  it('bad page ranges are rejected before any work', async () => {
    const s = setup([]);
    const j = await runJob(s.job('split-pdf', { ranges: '2-9' }, ['doc.pdf']), s.inputs(['doc.pdf', pdfB]), s.deps);
    expect([j.state, j.error?.code]).toEqual(['Rejected', 'Unsupported']);
  });

  it('locked PDF: asks for the password, stays asking on a wrong one, continues on the right one', async () => {
    const tried: string[] = [];
    const s = setup([], {
      unlock: async (_i, pw) => { tried.push(pw); return pw === 'right' ? { bytes: unlocked, format: 'pdf' } : 'wrong-password'; },
      run: async (_op, items) => items,
    }, ['nope', 'right']);
    const j = await runJob(s.job('compress-pdf', {}, ['l.pdf']), s.inputs(['l.pdf', locked]), s.deps);
    expect(tried).toEqual(['', 'nope', 'right']);
    expect(s.asked).toEqual([false, true]);
    expect(s.states).toEqual(['Probing', 'NeedsPassword', 'Probing', 'NeedsPassword', 'Probing', 'Running', 'Verifying', 'Saved']);
    expect(j.outputs?.[0]?.bytes).toBe(unlocked);
  });

  it('skipping a locked PDF ends Canceled with a notice, not Failed', async () => {
    const s = setup([], {}, [null]);
    const j = await runJob(s.job('compress-pdf', {}, ['l.pdf']), s.inputs(['l.pdf', locked]), s.deps);
    expect([j.state, j.error, j.notice]).toEqual(['Canceled', undefined, 'Skipped l.pdf: it is password-protected.']);
  });
});

describe('runJob: audio tools', () => {
  const song = fakeMp3();
  facts.set(key(song), { format: 'mp3', durationS: 30 });

  it('trim checks the requested times against the real length', async () => {
    const s = setup([]);
    const j = await runJob(s.job('trim-audio', { start: 40 }, ['s.mp3']), s.inputs(['s.mp3', song]), s.deps);
    expect([j.state, j.error?.why]).toEqual(['Rejected', 'the start (40 s) is after the end of the file (30.0 s)']);
  });
});

describe('verifyOutputs', () => {
  const dims = async (items: Item[]) => inspect(items);
  const exp = [{ format: 'jpeg' as const, width: 400, height: 300 }];
  const one = (bytes: Uint8Array): Item[] => [{ bytes, format: 'jpeg' }];
  it('rejects outputs that are empty, the wrong type, truncated, still carry EXIF, or too big', async () => {
    await expect(verifyOutputs(one(new Uint8Array()), exp, dims)).rejects.toThrow(/empty/);
    await expect(verifyOutputs(one(heic), exp, dims)).rejects.toThrow(/HEIC, not JPG/);
    await expect(verifyOutputs(one(fakeJpeg(400, 300, { truncated: true })), exp, dims)).rejects.toThrow(/ends early/);
    await expect(verifyOutputs(one(fakeJpeg(400, 300, { exif: true })), exp, dims)).rejects.toThrow(/EXIF/);
    await expect(verifyOutputs(one(fakeJpeg(400, 300)), [{ format: 'jpeg', maxBytes: 10 }], dims)).rejects.toThrow(/over the 0 KB target/);
    await expect(verifyOutputs(one(fakeJpeg(400, 300)), exp, dims)).resolves.toBeUndefined();
  });
});
