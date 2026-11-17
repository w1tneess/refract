import type { Format } from '../detect/detect.ts';

/** Minimal cancel signal; structurally compatible with the DOM AbortSignal. */
export interface CancelSignal {
  readonly aborted: boolean;
  addEventListener(type: 'abort', listener: () => void): void;
}

type Base = { key: string; label: string; help?: string; /** Show only when another option has this value. */ showIf?: { key: string; is: (string | number)[] } };
export type OptionSpec =
  | (Base & { type: 'number'; min: number; max: number; step?: number; unit?: string; default: number })
  | (Base & { type: 'select'; choices: { value: string; label: string }[]; default: string })
  | (Base & { type: 'text'; placeholder?: string; default: string });
export type Options = Record<string, number | string>;

/** A file moving through a job: its bytes and its (detected) format. */
export type Item = { bytes: Uint8Array; format: Format };

export type RunContext = {
  signal: CancelSignal;
  /** Call at least every few seconds while working; silence means the engine hung. */
  heartbeat: () => void;
  /** Record a timing (ms) on the job, e.g. worker load time. */
  report: (key: string, ms: number) => void;
};

export type Edge = {
  from: Format;
  to: Format;
  engine: 'wasm' | 'native'; // one edge per engine; two engines = two edges
  engineVersion: string; // pinned, stored in the job record
  options: OptionSpec[]; // drives the options UI
  lossy: boolean; // quality cost
  memFactor: number; // images: est. RAM = w × h × 4 × memFactor; others: input bytes × memFactor
  experimental?: boolean;
  run: (inputs: Item[], opts: Options, ctx: RunContext) => Promise<Item[]>;
};

const MAX_HOPS = 2;

/** Pick the route with the least quality loss, then the fewest hops. Reads only the edge table. */
export function planRoute(edges: Edge[], from: Format, to: Format, engines: Edge['engine'][]): Edge[] | null {
  const usable = edges.filter((e) => engines.includes(e.engine));
  const routes: Edge[][] = [];
  const walk = (at: Format, path: Edge[]) => {
    if (at === to && path.length) return void routes.push(path);
    if (path.length === MAX_HOPS) return;
    for (const e of usable) {
      // An intermediate step must be lossless (PNG, WAV); only the last hop may lose quality.
      if (e.from === at && !path.includes(e) && (e.to === to || !e.lossy)) walk(e.to, [...path, e]);
    }
  };
  walk(from, []);
  const cost = (r: Edge[]) => r.filter((e) => e.lossy).length * 10 + r.length;
  return routes.sort((a, b) => cost(a) - cost(b))[0] ?? null;
}

export const defaultOptions = (specs: OptionSpec[]): Options => Object.fromEntries(specs.map((o) => [o.key, o.default]));

/** Fill missing values with defaults and clamp numbers into range; unknown keys are dropped. */
export function resolveOptions(specs: OptionSpec[], given: Options = {}): Options {
  const out: Options = {};
  for (const s of specs) {
    const v = given[s.key];
    if (s.type === 'number') {
      const n = typeof v === 'number' && Number.isFinite(v) ? v : Number(v);
      out[s.key] = Number.isFinite(n) && v !== '' && v !== undefined ? Math.min(s.max, Math.max(s.min, n)) : s.default;
    } else if (s.type === 'select') {
      out[s.key] = s.choices.some((c) => c.value === v) ? (v as string) : s.default;
    } else {
      out[s.key] = typeof v === 'string' ? v : s.default;
    }
  }
  return out;
}
