import type { Format } from './detect/detect.ts';
import type { Edge, Item, Options, RunContext } from './graph/graph.ts';

/** What an engine can tell about a file by opening it with a real decoder. */
export type Inspect = {
  format: Format | 'unknown';
  width?: number;
  height?: number;
  pages?: number;
  durationS?: number;
  encrypted?: boolean;
};

/** Implemented by engine-wasm (and later engine-native). core never imports an engine. */
export interface Engine {
  kind: Edge['engine'];
  /** Format conversions, read by the route planner. */
  edges: Edge[];
  /** Run one operation (e.g. 'pdf.merge') on some files. */
  run: (op: string, inputs: Item[], opts: Options, ctx: RunContext) => Promise<Item[]>;
  /** Open files with a real decoder and report what is inside (used by Probe and Verify). */
  inspect: (items: Item[], ctx: RunContext) => Promise<Inspect[]>;
  /** Remove a PDF's password. Resolves 'wrong-password' when it doesn't open. */
  unlock: (item: Item, password: string, ctx: RunContext) => Promise<Item | 'wrong-password'>;
}
