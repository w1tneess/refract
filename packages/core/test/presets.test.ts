import { describe, expect, it } from 'vitest';
import { checkLimits, checkMemory, estimateImageMemory, limitsFor, outputName, CONVERTED, RefractError } from '../src/index.ts';

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
  it('skips names that are already taken further up', () => {
    const taken = new Set(['a (converted).jpg', 'a (converted) (2).jpg']);
    expect(outputName(CONVERTED, 'a.heic', 'jpg', taken)).toBe('a (converted) (3).jpg');
  });
  it('keeps Bengali, emoji, dots and extensionless names intact', () => {
    expect(outputName(CONVERTED, 'ছবি 😀.heic', 'jpg', new Set())).toBe('ছবি 😀 (converted).jpg');
    expect(outputName(CONVERTED, 'trip.v2.heic', 'jpg', new Set())).toBe('trip.v2 (converted).jpg');
    expect(outputName(CONVERTED, 'photo', 'jpg', new Set())).toBe('photo (converted).jpg');
    expect(outputName(CONVERTED, '.hidden', 'jpg', new Set())).toBe('.hidden (converted).jpg');
  });
});

describe('limits', () => {
  const web = limitsFor('image.convert', 'web');
  it('rejects a file over maxMB', () => {
    expect(code(() => checkLimits(web, { bytes: (web.maxMB + 1) * 1024 * 1024 }))).toBe('TooLarge');
    expect(code(() => checkLimits(web, { bytes: 1000 }))).toBe('ok');
  });
  it('rejects an image over maxPixels', () => {
    expect(code(() => checkLimits(web, { bytes: 1000, pixels: web.maxPixels! + 1 }))).toBe('TooLarge');
  });
  it('throws for an unknown operation', () => {
    expect(() => limitsFor('nope', 'web')).toThrow();
  });
});

describe('memory estimate', () => {
  it('is w × h × 4 × memFactor', () => {
    expect(estimateImageMemory(4032, 3024, 2.5)).toBe(4032 * 3024 * 4 * 2.5);
  });
  it('a 12 MP iPhone photo fits the web budget; a 120 MP one does not', () => {
    expect(code(() => checkMemory(estimateImageMemory(4032, 3024, 2.5), 'web'))).toBe('ok');
    expect(code(() => checkMemory(estimateImageMemory(12_000, 10_000, 2.5), 'web'))).toBe('TooLarge');
  });
});
