import { describe, expect, it } from 'vitest';
import {
  checkLimits, checkMemory, cmToPx, CONVERTED, limitsFor, outputName, parseOrder, parseRanges,
  resolveOptions, searchTargetSize, type OptionSpec, type RefractError,
} from '../src/index.ts';

const code = (fn: () => void) => {
  try { fn(); } catch (e) { return (e as RefractError).code; }
  return 'ok';
};

describe('outputName', () => {
  it('uses the template', () => {
    expect(outputName(CONVERTED, 'IMG_0001.HEIC', 'jpg', new Set())).toBe('IMG_0001 (converted).jpg');
  });
  it('adds (2), (3) for clashes within the same batch, ignoring case', () => {
    const taken = new Set<string>();
    expect(outputName(CONVERTED, 'IMG.HEIC', 'jpg', taken)).toBe('IMG (converted).jpg');
    expect(outputName(CONVERTED, 'IMG.HEIC', 'jpg', taken)).toBe('IMG (converted) (2).jpg');
    expect(outputName(CONVERTED, 'img.heic', 'jpg', taken)).toBe('img (converted) (3).jpg');
  });
  it('keeps Bengali, emoji, dots and extensionless names intact', () => {
    expect(outputName(CONVERTED, 'ছবি 😀.heic', 'jpg', new Set())).toBe('ছবি 😀 (converted).jpg');
    expect(outputName(CONVERTED, 'trip.v2.heic', 'jpg', new Set())).toBe('trip.v2 (converted).jpg');
    expect(outputName(CONVERTED, 'photo', 'jpg', new Set())).toBe('photo (converted).jpg');
    expect(outputName('{name}_pages', '.hidden', 'zip', new Set())).toBe('.hidden_pages.zip');
  });
});

describe('limits', () => {
  const web = limitsFor('image.convert', 'web');
  const pdf = limitsFor('pdf', 'web');
  it('rejects a file over maxMB, an image over maxPixels, a PDF over maxPages, too many files', () => {
    expect(code(() => checkLimits(web, { bytes: (web.maxMB + 1) * 1024 * 1024 }))).toBe('TooLarge');
    expect(code(() => checkLimits(web, { bytes: 1000, pixels: web.maxPixels! + 1 }))).toBe('TooLarge');
    expect(code(() => checkLimits(pdf, { bytes: 1000, pages: pdf.maxPages! + 1 }))).toBe('TooLarge');
    expect(code(() => checkLimits(pdf, { bytes: 0, files: pdf.maxFiles + 1 }))).toBe('TooLarge');
    expect(code(() => checkLimits(web, { bytes: 1000, pixels: 1000 }))).toBe('ok');
  });
  it('throws for an unknown operation', () => {
    expect(() => limitsFor('nope', 'web')).toThrow();
  });
  it('memory: a 12 MP photo fits the web budget; 120 MP does not', () => {
    expect(code(() => checkMemory(4032 * 3024 * 4 * 3, 'web'))).toBe('ok');
    expect(code(() => checkMemory(12_000 * 10_000 * 4 * 3, 'web'))).toBe('TooLarge');
  });
});

describe('page ranges', () => {
  it('empty means every page; ranges, single pages and open ends (7- and 7-end) work', () => {
    expect(parseRanges('', 3)).toEqual([[1], [2], [3]]);
    expect(parseRanges('1-3, 5, 7-', 8)).toEqual([[1, 2, 3], [5], [7, 8]]);
    expect(parseRanges('1-2, 6-end', 8)).toEqual([[1, 2], [6, 7, 8]]);
    expect(parseRanges('7-END', 8)).toEqual([[7, 8]]);
  });
  it('rejects bad syntax and pages that do not exist', () => {
    expect(code(() => parseRanges('a-b', 3))).toBe('Unsupported');
    expect(code(() => parseRanges('2-9', 3))).toBe('Unsupported');
    expect(code(() => parseRanges('3-1', 3))).toBe('Unsupported');
  });
  it('order must list every page exactly once', () => {
    expect(parseOrder('3, 1 2', 3)).toEqual([3, 1, 2]);
    expect(code(() => parseOrder('1, 2', 3))).toBe('Unsupported');
    expect(code(() => parseOrder('1, 1, 2', 3))).toBe('Unsupported');
    expect(code(() => parseOrder('1, 2, 4', 3))).toBe('Unsupported');
  });
});

describe('target size search', () => {
  // A fake encoder: size grows with quality and with the square of the scale.
  const fake = (q: number, s: number) => Promise.resolve(new Uint8Array(Math.round(q * 1000 * s * s)));
  it('returns the highest quality that fits', async () => {
    const r = await searchTargetSize(fake, 50_000);
    expect(r.bytes.length).toBeLessThanOrEqual(50_000);
    expect(r.quality).toBe(50);
    expect(r.scale).toBe(1);
  });
  it('shrinks the image when even the lowest quality is too big', async () => {
    const r = await searchTargetSize(fake, 8_000);
    expect(r.bytes.length).toBeLessThanOrEqual(8_000);
    expect(r.scale).toBeLessThan(1);
  });
  it('fails with a clear reason when nothing fits', async () => {
    await expect(searchTargetSize(fake, 10)).rejects.toMatchObject({ code: 'TooLarge' });
  });
  it('cm to px at a DPI', () => {
    expect(cmToPx(2.54, 300)).toBe(300);
    expect(cmToPx(3.5, 200)).toBe(276);
  });
});

describe('resolveOptions', () => {
  const specs: OptionSpec[] = [
    { key: 'q', label: 'Q', type: 'number', min: 10, max: 100, default: 90 },
    { key: 'to', label: 'To', type: 'select', choices: [{ value: 'png', label: 'PNG' }], default: 'png' },
    { key: 'r', label: 'R', type: 'text', default: '' },
  ];
  it('fills defaults, clamps numbers, rejects unknown choices and drops unknown keys', () => {
    expect(resolveOptions(specs, { q: 500, to: 'bmp', extra: 1 })).toEqual({ q: 100, to: 'png', r: '' });
    expect(resolveOptions(specs, { q: '42', r: '1-2' })).toEqual({ q: 42, to: 'png', r: '1-2' });
  });
});
