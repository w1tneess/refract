import { RefractError } from '../errors.ts';

export type TargetResult = { bytes: Uint8Array; quality: number; scale: number };

/**
 * Find the best quality whose output fits `targetBytes` (the brief's algorithm):
 * binary-search quality 10–95 up to 8 times; if even the lowest misses, scale down 10% and repeat (max 5 times).
 * `encode(quality, scale)` must be deterministic.
 */
export async function searchTargetSize(
  encode: (quality: number, scale: number) => Promise<Uint8Array>,
  targetBytes: number,
  onTry?: () => void,
): Promise<TargetResult> {
  let scale = 1;
  for (let shrink = 0; shrink <= 5; shrink++) {
    let lo = 10;
    let hi = 95;
    let best: TargetResult | undefined;
    for (let i = 0; i < 8 && lo <= hi; i++) {
      const q = Math.floor((lo + hi) / 2);
      const out = await encode(q, scale);
      onTry?.();
      if (out.length <= targetBytes) {
        best = { bytes: out, quality: q, scale };
        lo = q + 1;
      } else {
        hi = q - 1;
      }
    }
    if (best) return best;
    scale *= 0.9;
  }
  throw new RefractError(
    'TooLarge',
    `even at the lowest quality and ${Math.round(scale * 100 / 0.9)}% of its size, the image is bigger than ${Math.round(targetBytes / 1024)} KB`,
    'Pick a larger target size, or resize the image first.',
  );
}

/** Centimetres to pixels at a given DPI. */
export const cmToPx = (cm: number, dpi: number) => Math.round((cm / 2.54) * dpi);
