import { RefractError } from '../errors.ts';

export type Limits = { maxMB: number; maxFiles: number; maxPages?: number; maxPixels?: number };
export type Platform = 'web' | 'desktop';

const MiB = 1024 * 1024;

/** Limits per operation, checked in Probe before any work starts. */
export const LIMITS: Record<string, Record<Platform, Limits>> = {
  'image.convert': {
    web: { maxMB: 200, maxFiles: 200, maxPixels: 100_000_000 },
    desktop: { maxMB: 2048, maxFiles: 10_000, maxPixels: 1_000_000_000 },
  },
};

// shortcut: one fixed per-job budget. Upgrade to navigator.deviceMemory-based budgets once phones are tested.
export const MEMORY_BUDGET_BYTES: Record<Platform, number> = { web: 1024 * MiB, desktop: 8192 * MiB };

const mb = (bytes: number) => Math.ceil(bytes / MiB);

export function limitsFor(op: string, platform: Platform): Limits {
  const l = LIMITS[op]?.[platform];
  if (!l) throw new Error(`No limits defined for ${op} on ${platform}`);
  return l;
}

/** Throws TooLarge when a file breaks the operation's limits. */
export function checkLimits(l: Limits, input: { bytes: number; pixels?: number }): void {
  if (input.bytes > l.maxMB * MiB) {
    throw new RefractError('TooLarge', `the file is ${mb(input.bytes)} MB and the limit here is ${l.maxMB} MB`, 'Try a smaller file.');
  }
  if (l.maxPixels !== undefined && input.pixels !== undefined && input.pixels > l.maxPixels) {
    const mp = (n: number) => Math.round(n / 1e6);
    throw new RefractError('TooLarge', `the image is ${mp(input.pixels)} megapixels and the limit here is ${mp(l.maxPixels)}`, 'Try a smaller image.');
  }
}

/** Decoded-image memory estimate: w × h × 4 bytes of RGBA, times the route's working-copy factor. */
export function estimateImageMemory(width: number, height: number, memFactor: number): number {
  return width * height * 4 * memFactor;
}

export function checkMemory(estimate: number, platform: Platform): void {
  const budget = MEMORY_BUDGET_BYTES[platform];
  if (estimate > budget) {
    throw new RefractError(
      'TooLarge',
      `it needs about ${mb(estimate)} MB of memory to convert and the browser budget is ${mb(budget)} MB`,
      'Use a smaller image, or the desktop app once it is available.',
    );
  }
}
