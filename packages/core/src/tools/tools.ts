import { ext, FORMATS, type Format, type Kind } from '../detect/detect.ts';
import type { ImageProbe } from '../detect/image.ts';
import type { Engine } from '../engine.ts';
import { RefractError } from '../errors.ts';
import {
  manifestOptionsForPlatform,
  TOOL_MANIFEST,
  type ManifestOptionSpec,
  type ToolId,
} from '../generated/toolManifest.ts';
import { planRoute, type Item, type OptionSpec, type Options, type RunContext } from '../graph/graph.ts';
import { parseOrder, parseRanges } from '../presets/pages.ts';
import { outputName } from '../presets/naming.ts';
import { cmToPx } from '../presets/targetSize.ts';

export type { ToolId };

/** One input after Probe. */
export type Probed = {
  name: string;
  item: Item;
  image?: ImageProbe;
  pages?: number;
  durationS?: number;
};

/** What Verify checks on each output. Undefined fields are not checked. */
export type Expect = { format: Format; width?: number; height?: number; pages?: number; durationS?: number; maxBytes?: number };

export type PlannedStep = { op: string; engineVersion?: string; opts: Options; run: (items: Item[], ctx: RunContext) => Promise<Item[]> };

export type Plan = {
  steps: PlannedStep[];
  expect: Expect[];
  /** Names of the files inside the ZIP, for tools with several outputs. */
  entryNames?: string[];
  /** A note shown on the card after the job, e.g. "already as small as it gets". */
  notice?: (outs: Item[]) => string | undefined;
};

export type Tool = {
  id: ToolId;
  label: string;
  blurb: string;
  accepts: Kind;
  inputs: 'one' | 'many';
  minInputs?: number;
  options: OptionSpec[];
  /** Key into presets/limits.ts. */
  limits: string;
  /** Output name pieces, decided when the job is created (format guessed from the file name). */
  output: (opts: Options, inFormat: Format | undefined) => { template: string; ext: string; multi: boolean };
  /** Peak memory estimate in bytes, checked before any work. */
  memory: (inputs: Probed[], opts: Options) => number;
  plan: (inputs: Probed[], opts: Options, engine: Engine) => Plan;
};

const pixels = (p: Probed) => (p.image ? p.image.width * p.image.height : 0);
const maxPixels = (ins: Probed[]) => Math.max(0, ...ins.map(pixels));
const totalBytes = (ins: Probed[]) => ins.reduce((n, p) => n + p.item.bytes.length, 0);
const unsupported = (why: string, todo: string) => new RefractError('Unsupported', why, todo);
/** HEIC can be read but not written; tools that keep the input format write JPG instead. */
const sameOrJpeg = (f: Format | undefined): Format => (f === 'heic' || !f ? 'jpeg' : f);
const op = (engine: Engine, name: string, opts: Options): PlannedStep => ({
  op: name, opts, run: (items, ctx) => engine.run(name, items, opts, ctx),
});

function toOptionSpec(spec: ManifestOptionSpec): OptionSpec {
  const showIf = spec.showIf ? { key: spec.showIf.key, is: [...spec.showIf.is] } : undefined;
  if (spec.type === 'select') {
    return {
      key: spec.key,
      label: spec.label,
      type: 'select',
      choices: spec.choices.map((c) => ({ value: c.value, label: c.label })),
      default: spec.default,
      ...(spec.help ? { help: spec.help } : {}),
      ...(showIf ? { showIf } : {}),
    };
  }
  if (spec.type === 'number') {
    return {
      key: spec.key,
      label: spec.label,
      type: 'number',
      min: spec.min,
      max: spec.max,
      default: spec.default,
      ...(spec.step !== undefined ? { step: spec.step } : {}),
      ...(spec.unit ? { unit: spec.unit } : {}),
      ...(spec.help ? { help: spec.help } : {}),
      ...(showIf ? { showIf } : {}),
    };
  }
  return {
    key: spec.key,
    label: spec.label,
    type: 'text',
    default: spec.default,
    ...(spec.placeholder !== undefined ? { placeholder: spec.placeholder } : {}),
    ...(spec.help ? { help: spec.help } : {}),
    ...(showIf ? { showIf } : {}),
  };
}

function fromManifest(id: ToolId, runtime: Pick<Tool, 'memory' | 'plan'>): Tool {
  const spec = TOOL_MANIFEST[id];
  const { template, multi, extRule, fixedExt } = spec.outputNaming;
  return {
    id,
    label: spec.label,
    blurb: spec.blurb,
    accepts: spec.acceptsKind,
    inputs: spec.inputs.mode,
    ...(spec.inputs.min > 1 || spec.inputs.mode === 'many' ? { minInputs: spec.inputs.min } : {}),
    limits: spec.limitsKey,
    options: manifestOptionsForPlatform(id, 'web').map(toOptionSpec),
    output: (o, f) => {
      let outExt: string;
      if (extRule === 'fromOptionTo') {
        outExt = ext(o.to as Format);
      } else if (extRule === 'sameOrJpeg') {
        outExt = ext(o.to === 'same' ? sameOrJpeg(f) : (o.to as Format));
      } else if (extRule === 'sameAudioOrMp3') {
        outExt = ext(f && FORMATS[f].kind === 'audio' ? f : 'mp3');
      } else {
        outExt = fixedExt ?? 'bin';
      }
      return { template, ext: outExt, multi };
    },
    ...runtime,
  };
}

/** A format conversion through the edge table (route planner), shared by image and audio "Convert". */
function convertPlan(input: Probed, to: Format, opts: Options, engine: Engine, expect: Expect): Plan {
  const from = input.item.format;
  if (from === to) {
    throw unsupported(`it is already a ${FORMATS[to].label} file`, `Pick a different format${FORMATS[to].kind === 'image' ? ', or use Compress image' : ''}.`);
  }
  const route = planRoute(engine.edges, from, to, [engine.kind]);
  if (!route) throw unsupported(`Refract can't convert ${FORMATS[from].label} to ${FORMATS[to].label} yet`, 'Pick a different format.');
  return {
    steps: route.map((e) => ({
      op: `${e.from}→${e.to}`,
      engineVersion: e.engineVersion,
      opts,
      run: (items: Item[], ctx: RunContext) => e.run(items, opts, ctx),
    })),
    expect: [expect],
  };
}

export const TOOLS: Record<ToolId, Tool> = {
  'convert-image': fromManifest('convert-image', {
    memory: (ins) => maxPixels(ins) * 4 * 3,
    plan: ([p], o, engine) => convertPlan(p!, o.to as Format, o, engine, { format: o.to as Format, width: p!.image!.width, height: p!.image!.height }),
  }),

  'compress-image': fromManifest('compress-image', {
    memory: (ins) => maxPixels(ins) * 4 * 4,
    plan: ([p], o, engine) => ({
      steps: [op(engine, 'image.target', { to: o.to!, targetBytes: Number(o.targetKB) * 1024 })],
      // The size may shrink to reach the target, so only the type and byte limit are checked.
      expect: [{ format: o.to as Format, maxBytes: Number(o.targetKB) * 1024 }],
      notice: ([out]) => (out && out.bytes.length >= p!.item.bytes.length ? 'The result is not smaller than the original.' : undefined),
    }),
  }),

  'resize-image': fromManifest('resize-image', {
    memory: (ins, o) => {
      const target = ins[0]?.image ? resolveResizeDimensions(ins[0].image, o) : null;
      const targetPixels = target ? target.w * target.h : 0;
      return Math.max(maxPixels(ins), targetPixels) * 4 * 4;
    },
    plan: ([p], o, engine) => {
      const dims = resolveResizeDimensions(p!.image!, o);
      if (!dims) throw unsupported('no width or height was given', 'Enter a width, a height, or both.');
      const { w, h } = dims;
      const to = o.to === 'same' ? sameOrJpeg(p!.item.format) : (o.to as Format);
      return { steps: [op(engine, 'image.convert', { to, quality: 90, width: w, height: h })], expect: [{ format: to, width: w, height: h }] };
    },
  }),

  'merge-pdf': fromManifest('merge-pdf', {
    memory: (ins) => totalBytes(ins) * 5,
    plan: (ins, _o, engine) => ({
      steps: [op(engine, 'pdf.merge', {})],
      expect: [{ format: 'pdf', pages: ins.reduce((n, p) => n + p.pages!, 0) }],
    }),
  }),

  'split-pdf': fromManifest('split-pdf', {
    memory: (ins) => totalBytes(ins) * 5,
    plan: ([p], o, engine) => {
      const groups = parseRanges(String(o.ranges), p!.pages!);
      const stem = stemOf(p!.name);
      const entryTaken = new Set<string>();
      return {
        steps: [op(engine, 'pdf.split', { groups: JSON.stringify(groups) })],
        expect: groups.map((g) => ({ format: 'pdf', pages: g.length })),
        entryNames: groups.map((g) => {
          const suffix = g.length > 1 ? `${g[0]}-${g.at(-1)}` : `${g[0]}`;
          return outputName(TOOL_MANIFEST['split-pdf'].outputNaming.entryTemplate!.replace('{range}', suffix).replace(/\.pdf$/, ''), `${stem}.pdf`, 'pdf', entryTaken);
        }),
      };
    },
  }),

  'reorder-pdf': fromManifest('reorder-pdf', {
    memory: (ins) => totalBytes(ins) * 5,
    plan: ([p], o, engine) => {
      const order = parseOrder(String(o.order), p!.pages!);
      return { steps: [op(engine, 'pdf.reorder', { order: JSON.stringify(order) })], expect: [{ format: 'pdf', pages: order.length }] };
    },
  }),

  'images-to-pdf': fromManifest('images-to-pdf', {
    memory: (ins) => maxPixels(ins) * 4 * 3 + totalBytes(ins) * 2,
    plan: (ins, o, engine) => ({
      steps: [op(engine, 'pdf.fromImages', { pageSize: o.pageSize!, orientations: JSON.stringify(ins.map((p) => p.image?.orientation ?? 1)) })],
      expect: [{ format: 'pdf', pages: ins.length }],
    }),
  }),

  'pdf-to-images': fromManifest('pdf-to-images', {
    // One page is rendered at a time; estimate an A4 page at the chosen DPI.
    memory: (ins, o) => totalBytes(ins) * 5 + 8.27 * 11.69 * Number(o.dpi) ** 2 * 4 * 3,
    plan: ([p], o, engine) => {
      const stem = stemOf(p!.name);
      const n = p!.pages!;
      return {
        steps: [op(engine, 'pdf.toImages', { to: o.to!, dpi: o.dpi! })],
        expect: Array.from({ length: n }, () => ({ format: o.to as Format })),
        entryNames: Array.from({ length: n }, (_, i) => `${stem}-p${String(i + 1).padStart(String(n).length, '0')}.${ext(o.to as Format)}`),
      };
    },
  }),

  'compress-pdf': fromManifest('compress-pdf', {
    memory: (ins) => totalBytes(ins) * 5,
    plan: ([p], _o, engine) => ({
      steps: [op(engine, 'pdf.compress', {})],
      expect: [{ format: 'pdf', pages: p!.pages! }],
      notice: ([out]) =>
        out && out.bytes.length >= p!.item.bytes.length
          ? 'This PDF is already as small as lossless compression can make it, so the file is unchanged.'
          : undefined,
    }),
  }),

  'convert-audio': fromManifest('convert-audio', {
    memory: (ins) => totalBytes(ins) * 4 + (ins[0]?.durationS ?? 0) * 48_000 * 2 * 4 * 2,
    plan: ([p], o, engine) => convertPlan(p!, o.to as Format, o, engine, { format: o.to as Format, durationS: p!.durationS! }),
  }),

  'trim-audio': fromManifest('trim-audio', {
    memory: (ins) => totalBytes(ins) * 4 + (ins[0]?.durationS ?? 0) * 48_000 * 2 * 4 * 2,
    plan: ([p], o, engine) => {
      const d = p!.durationS!;
      const start = Number(o.start);
      const end = Number(o.end) || d;
      if (start >= d) throw unsupported(`the start (${start} s) is after the end of the file (${d.toFixed(1)} s)`, 'Pick an earlier start.');
      if (end <= start) throw unsupported('the end is before the start', 'Pick an end time after the start.');
      const stop = Math.min(end, d);
      return {
        steps: [op(engine, 'audio.trim', { start, end: stop, to: p!.item.format, bitrate: o.bitrate! })],
        expect: [{ format: p!.item.format, durationS: stop - start }],
      };
    },
  }),
};

function resolveResizeDimensions(src: { width: number; height: number }, o: Options): { w: number; h: number } | null {
  const size = (v: number) => (o.unit === 'cm' ? cmToPx(v, Number(o.dpi)) : Math.round(v));
  let w = size(Number(o.width));
  let h = size(Number(o.height));
  if (!w && !h) return null;
  if (!w) w = Math.max(1, Math.round((h * src.width) / src.height));
  if (!h) h = Math.max(1, Math.round((w * src.height) / src.width));
  return { w, h };
}

export function stemOf(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(0, dot) : name;
}
