import { ext, FORMATS, formatsOfKind, type Format, type Kind } from '../detect/detect.ts';
import type { ImageProbe } from '../detect/image.ts';
import type { Engine } from '../engine.ts';
import { RefractError } from '../errors.ts';
import { planRoute, type Item, type OptionSpec, type Options, type RunContext } from '../graph/graph.ts';
import { parseOrder, parseRanges } from '../presets/pages.ts';
import { outputName } from '../presets/naming.ts';
import { cmToPx } from '../presets/targetSize.ts';

export type ToolId =
  | 'convert-image' | 'compress-image' | 'resize-image'
  | 'merge-pdf' | 'split-pdf' | 'reorder-pdf' | 'images-to-pdf' | 'pdf-to-images' | 'compress-pdf'
  | 'convert-audio' | 'trim-audio';

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

const IMAGE_OUT = ['jpeg', 'png', 'webp'] as const;
const choice = (f: Format) => ({ value: f, label: FORMATS[f].label });
const quality: OptionSpec = {
  key: 'quality', label: 'Quality', type: 'number', min: 10, max: 100, default: 90, unit: '%',
  showIf: { key: 'to', is: ['jpeg', 'webp'] },
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
  'convert-image': {
    id: 'convert-image', label: 'Convert image', blurb: 'HEIC, WebP, PNG and JPG to each other.',
    accepts: 'image', inputs: 'one', limits: 'image.convert',
    options: [
      { key: 'to', label: 'Convert to', type: 'select', choices: IMAGE_OUT.map(choice), default: 'jpeg' },
      quality,
    ],
    output: (o) => ({ template: '{name} (converted)', ext: ext(o.to as Format), multi: false }),
    memory: (ins) => maxPixels(ins) * 4 * 3,
    plan: ([p], o, engine) => convertPlan(p!, o.to as Format, o, engine, { format: o.to as Format, width: p!.image!.width, height: p!.image!.height }),
  },

  'compress-image': {
    id: 'compress-image', label: 'Compress image', blurb: 'Shrink a photo to a target size in KB.',
    accepts: 'image', inputs: 'one', limits: 'image.convert',
    options: [
      { key: 'to', label: 'Save as', type: 'select', choices: [choice('jpeg'), choice('webp')], default: 'jpeg' },
      { key: 'targetKB', label: 'Target size', type: 'number', min: 5, max: 20000, default: 200, unit: 'KB' },
    ],
    output: (o) => ({ template: '{name}_compressed', ext: ext(o.to as Format), multi: false }),
    memory: (ins) => maxPixels(ins) * 4 * 4,
    plan: ([p], o, engine) => ({
      steps: [op(engine, 'image.target', { to: o.to!, targetBytes: Number(o.targetKB) * 1024 })],
      // The size may shrink to reach the target, so only the type and byte limit are checked.
      expect: [{ format: o.to as Format, maxBytes: Number(o.targetKB) * 1024 }],
      notice: ([out]) => (out && out.bytes.length >= p!.item.bytes.length ? 'The result is not smaller than the original.' : undefined),
    }),
  },

  'resize-image': {
    id: 'resize-image', label: 'Resize image', blurb: 'Set the size in pixels or centimetres.',
    accepts: 'image', inputs: 'one', limits: 'image.convert',
    options: [
      { key: 'unit', label: 'Unit', type: 'select', choices: [{ value: 'px', label: 'Pixels' }, { value: 'cm', label: 'Centimetres' }], default: 'px' },
      { key: 'width', label: 'Width', type: 'number', min: 0, max: 20000, step: 0.1, default: 1200, help: '0 keeps the aspect ratio' },
      { key: 'height', label: 'Height', type: 'number', min: 0, max: 20000, step: 0.1, default: 0, help: '0 keeps the aspect ratio' },
      { key: 'dpi', label: 'Print resolution', type: 'number', min: 72, max: 1200, default: 300, unit: 'DPI', showIf: { key: 'unit', is: ['cm'] } },
      { key: 'to', label: 'Save as', type: 'select', choices: [{ value: 'same', label: 'Same format' }, ...IMAGE_OUT.map(choice)], default: 'same' },
    ],
    output: (o, f) => ({ template: '{name}_resized', ext: ext(o.to === 'same' ? sameOrJpeg(f) : (o.to as Format)), multi: false }),
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
  },

  'merge-pdf': {
    id: 'merge-pdf', label: 'Merge PDFs', blurb: 'Join several PDFs into one, in the order added.',
    accepts: 'pdf', inputs: 'many', minInputs: 2, limits: 'pdf', options: [],
    output: () => ({ template: '{name}_merged', ext: 'pdf', multi: false }),
    memory: (ins) => totalBytes(ins) * 5,
    plan: (ins, _o, engine) => ({
      steps: [op(engine, 'pdf.merge', {})],
      expect: [{ format: 'pdf', pages: ins.reduce((n, p) => n + p.pages!, 0) }],
    }),
  },

  'split-pdf': {
    id: 'split-pdf', label: 'Split PDF', blurb: 'One file per page, or per range like 1-3, 5, 8-.',
    accepts: 'pdf', inputs: 'one', limits: 'pdf',
    options: [{ key: 'ranges', label: 'Page ranges', type: 'text', default: '', placeholder: 'Every page', help: 'e.g. 1-3, 5, 8-' }],
    output: () => ({ template: '{name}_split', ext: 'zip', multi: true }),
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
          return outputName(`{name}-p${suffix}`, `${stem}.pdf`, 'pdf', entryTaken);
        }),
      };
    },
  },

  'reorder-pdf': {
    id: 'reorder-pdf', label: 'Reorder pages', blurb: 'Put the pages of a PDF in a new order.',
    accepts: 'pdf', inputs: 'one', limits: 'pdf',
    options: [{ key: 'order', label: 'New page order', type: 'text', default: '', placeholder: 'e.g. 3, 1, 2' }],
    output: () => ({ template: '{name}_reordered', ext: 'pdf', multi: false }),
    memory: (ins) => totalBytes(ins) * 5,
    plan: ([p], o, engine) => {
      const order = parseOrder(String(o.order), p!.pages!);
      return { steps: [op(engine, 'pdf.reorder', { order: JSON.stringify(order) })], expect: [{ format: 'pdf', pages: order.length }] };
    },
  },

  'images-to-pdf': {
    id: 'images-to-pdf', label: 'Images to PDF', blurb: 'One page per image, in the order added.',
    accepts: 'image', inputs: 'many', minInputs: 1, limits: 'image.convert',
    options: [{
      key: 'pageSize', label: 'Page size', type: 'select', default: 'a4',
      choices: [{ value: 'a4', label: 'A4' }, { value: 'letter', label: 'Letter' }, { value: 'fit', label: 'Same as image' }],
    }],
    output: () => ({ template: '{name}', ext: 'pdf', multi: false }),
    memory: (ins) => maxPixels(ins) * 4 * 3 + totalBytes(ins) * 2,
    plan: (ins, o, engine) => ({
      steps: [op(engine, 'pdf.fromImages', { pageSize: o.pageSize!, orientations: JSON.stringify(ins.map((p) => p.image?.orientation ?? 1)) })],
      expect: [{ format: 'pdf', pages: ins.length }],
    }),
  },

  'pdf-to-images': {
    id: 'pdf-to-images', label: 'PDF to images', blurb: 'Each page as a PNG or JPG, saved as a ZIP.',
    accepts: 'pdf', inputs: 'one', limits: 'pdf',
    options: [
      { key: 'to', label: 'Image format', type: 'select', choices: [choice('png'), choice('jpeg')], default: 'png' },
      { key: 'dpi', label: 'Resolution', type: 'number', min: 36, max: 300, default: 150, unit: 'DPI' },
    ],
    output: () => ({ template: '{name}_pages', ext: 'zip', multi: true }),
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
  },

  'compress-pdf': {
    id: 'compress-pdf', label: 'Compress PDF', blurb: 'Lossless: nothing in the document changes.',
    accepts: 'pdf', inputs: 'one', limits: 'pdf', options: [],
    output: () => ({ template: '{name}_compressed', ext: 'pdf', multi: false }),
    memory: (ins) => totalBytes(ins) * 5,
    plan: ([p], _o, engine) => ({
      steps: [op(engine, 'pdf.compress', {})],
      expect: [{ format: 'pdf', pages: p!.pages! }],
      notice: ([out]) =>
        out && out.bytes.length >= p!.item.bytes.length
          ? 'This PDF is already as small as lossless compression can make it, so the file is unchanged.'
          : undefined,
    }),
  },

  'convert-audio': {
    id: 'convert-audio', label: 'Convert audio', blurb: 'MP3, WAV, M4A and OGG to each other.',
    accepts: 'audio', inputs: 'one', limits: 'audio',
    options: [
      { key: 'to', label: 'Convert to', type: 'select', choices: formatsOfKind('audio').map(choice), default: 'mp3' },
      bitrate(),
    ],
    output: (o) => ({ template: '{name} (converted)', ext: ext(o.to as Format), multi: false }),
    memory: (ins) => totalBytes(ins) * 4 + (ins[0]?.durationS ?? 0) * 48_000 * 2 * 4 * 2,
    plan: ([p], o, engine) => convertPlan(p!, o.to as Format, o, engine, { format: o.to as Format, durationS: p!.durationS! }),
  },

  'trim-audio': {
    id: 'trim-audio', label: 'Trim audio', blurb: 'Keep only the part between two times.',
    accepts: 'audio', inputs: 'one', limits: 'audio',
    options: [
      { key: 'start', label: 'Start', type: 'number', min: 0, max: 86_400, step: 0.1, default: 0, unit: 's' },
      { key: 'end', label: 'End', type: 'number', min: 0, max: 86_400, step: 0.1, default: 0, unit: 's', help: '0 means the end of the file' },
      { ...bitrate(), showIf: undefined, help: 'Not used for WAV' },
    ],
    output: (_o, f) => ({ template: '{name}_trimmed', ext: ext(f && FORMATS[f].kind === 'audio' ? f : 'mp3'), multi: false }),
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
  },
};

function bitrate(): OptionSpec {
  return {
    key: 'bitrate', label: 'Bitrate', type: 'select', default: '192k',
    choices: ['128k', '192k', '256k', '320k'].map((v) => ({ value: v, label: `${parseInt(v)} kbps` })),
    showIf: { key: 'to', is: ['mp3', 'm4a', 'ogg'] },
  };
}

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
