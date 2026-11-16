import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { detect, extensionMismatch, probeHeif, RefractError } from '../src/index.ts';

const fixture = (name: string) => new Uint8Array(readFileSync(new URL(`../../../fixtures/${name}`, import.meta.url)));
const ftyp = (brand: string) => new Uint8Array([0, 0, 0, 24, ...Buffer.from('ftyp' + brand), 0, 0, 0, 0]);

describe('detect (magic bytes)', () => {
  it.each(['heic', 'heix', 'mif1'])('ftyp%s is HEIC', (brand) => expect(detect(ftyp(brand))).toBe('heic'));
  it('FF D8 FF is JPEG', () => expect(detect(new Uint8Array([0xff, 0xd8, 0xff, 0xe0]))).toBe('jpeg'));
  it('other ftyp brands and short or empty input are unknown', () => {
    expect(detect(ftyp('isom'))).toBe('unknown');
    expect(detect(new Uint8Array([0xff, 0xd8]))).toBe('unknown');
    expect(detect(new Uint8Array())).toBe('unknown');
  });
  it('ignores the extension: a .jpg that is really HEIC is HEIC', () => {
    expect(detect(fixture('really-heic.jpg'))).toBe('heic');
  });
});

describe('extensionMismatch', () => {
  it('says so when the extension disagrees', () => {
    expect(extensionMismatch('really-heic.jpg', 'heic')).toMatch(/named \.jpg, but it is really a HEIC image/);
    expect(extensionMismatch('noext', 'heic')).toMatch(/has no extension/);
  });
  it('is quiet when it agrees, in any case', () => {
    expect(extensionMismatch('IMG_0001.HEIC', 'heic')).toBeNull();
    expect(extensionMismatch('a.heif', 'heic')).toBeNull();
  });
});

describe('probeHeif', () => {
  it('reads the size of a plain HEIC', () => {
    expect(probeHeif(fixture('plain.heic'))).toEqual({ width: 400, height: 300, rotation: 0, mirrored: false });
  });
  it('applies irot to the reported size of a rotated HEIC', () => {
    const p = probeHeif(fixture('rotated.heic'));
    expect([p.width, p.height]).toEqual([300, 400]);
    expect(p.rotation).toBe(270); // EXIF orientation 6 (90° clockwise) = irot 270° anti-clockwise
  });
  it('passes a file truncated inside mdat (decode will fail later)', () => {
    expect(probeHeif(fixture('truncated.heic')).width).toBe(400);
  });
  it('throws Corrupt when the metadata is cut off', () => {
    const cut = fixture('plain.heic').subarray(0, 60);
    expect(() => probeHeif(cut)).toThrow(RefractError);
    try { probeHeif(cut); } catch (e) { expect((e as RefractError).code).toBe('Corrupt'); }
  });
});
