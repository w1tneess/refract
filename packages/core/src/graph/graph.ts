import type { Format } from '../detect/detect.ts';

/** Minimal cancel signal; structurally compatible with the DOM AbortSignal. */
export interface CancelSignal {
  readonly aborted: boolean;
  addEventListener(type: 'abort', listener: () => void): void;
}

export type OptionSpec = { key: string; label: string; type: 'number'; min: number; max: number; default: number };
export type Options = Record<string, number>;

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
  memFactor: number; // images: est. RAM = w × h × 4 × memFactor
  experimental?: boolean;
  run: (input: Uint8Array, opts: Options, ctx: RunContext) => Promise<Uint8Array>;
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

export const defaultOptions = (e: Edge): Options => Object.fromEntries(e.options.map((o) => [o.key, o.default]));
