import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { detect, extensionMismatch, formatFromName, probeHeif, probeImage, jpegOrientation, RefractError } from '../src/index.ts';
import { fakeJpeg, fakePng, fakeWebp } from './helpers.ts';

const fixture = (name: string) => new Uint8Array(readFileSync(new URL(`../../../fixtures/${name}`, import.meta.url)));
const ascii = (s: string) => [...s].map((c) => c.charCodeAt(0));
const ftyp = (brand: string) => new Uint8Array([0, 0, 0, 24, ...ascii('ftyp' + brand), 0, 0, 0, 0]);
const riff = (kind: string) => new Uint8Array([...ascii('RIFF'), 0, 0, 0, 0, ...ascii(kind), 0, 0, 0, 0]);

describe('detect (magic bytes)', () => {
  it.each(['heic', 'heix', 'mif1'])('ftyp%s is HEIC', (brand) => expect(detect(ftyp(brand))).toBe('heic'));
  it.each([
    ['jpeg', new Uint8Array([0xff, 0xd8, 0xff, 0xe0])],
    ['png', fakePng(1, 1)],
    ['webp', riff('WEBP')],
    ['wav', riff('WAVE')],
    ['m4a', ftyp('M4A ')],
    ['ogg', new Uint8Array([...ascii('OggS'), 0, 2])],
    ['pdf', new TextEncoder().encode('%PDF-1.4\n')],
    ['pdf', new TextEncoder().encode('junk before %PDF-1.4\n')],
    ['mp3', new Uint8Array([...ascii('ID3'), 4, 0])],
    ['mp3', new Uint8Array([0xff, 0xfb, 0x90, 0x00])],
  ] as const)('%s', (fmt, bytes) => expect(detect(bytes)).toBe(fmt));
  it('ADTS AAC frame sync is not MP3; other ftyp brands, short and empty input are unknown', () => {
    expect(detect(new Uint8Array([0xff, 0xf1, 0x50, 0x80]))).toBe('unknown');
    expect(detect(ftyp('avif'))).toBe('unknown');
    expect(detect(new Uint8Array([0xff, 0xd8]))).toBe('unknown');
    expect(detect(new Uint8Array())).toBe('unknown');
  });
  it('ignores the extension: a .jpg that is really HEIC is HEIC', () => {
    expect(detect(fixture('really-heic.jpg'))).toBe('heic');
  });
});

describe('names', () => {
  it('extensionMismatch says so when the extension disagrees, and is quiet when it agrees', () => {
    expect(extensionMismatch('really-heic.jpg', 'heic')).toMatch(/named \.jpg, but it is really a HEIC image/);
    expect(extensionMismatch('noext', 'pdf')).toMatch(/has no extension, but it is really a PDF document/);
    expect(extensionMismatch('IMG_0001.HEIC', 'heic')).toBeNull();
    expect(extensionMismatch('song.MP3', 'mp3')).toBeNull();
  });
  it('formatFromName guesses only from the extension', () => {
    expect(formatFromName('a.JPEG')).toBe('jpeg');
    expect(formatFromName('a.unknown')).toBeUndefined();
  });
});

describe('probeHeif', () => {
  it('reads the size of a plain HEIC', () => {
    expect(probeHeif(fixture('plain.heic'))).toEqual({ width: 400, height: 300, rotation: 0, mirrored: false });
  });
  it('applies irot to the reported size of a rotated HEIC', () => {
    const p = probeHeif(fixture('rotated.heic'));
    expect([p.width, p.height, p.rotation]).toEqual([300, 400, 270]);
  });
  it('reads a real-phone grid HEIC', () => {
    expect(probeImage('heic', fixture('grid-real.heic'))).toMatchObject({ width: 4242, height: 2828 });
  });
  it('passes a file truncated inside mdat (decode will fail later)', () => {
    expect(probeHeif(fixture('truncated.heic')).width).toBe(400);
  });
  it('throws Corrupt when the metadata is cut off', () => {
    const cut = fixture('plain.heic').subarray(0, 60);
    try { probeHeif(cut); expect.unreachable(); } catch (e) { expect((e as RefractError).code).toBe('Corrupt'); }
  });
});

describe('probeImage', () => {
  it('PNG: size and alpha from IHDR', () => {
    expect(probeImage('png', fakePng(640, 480, 6))).toEqual({ width: 640, height: 480, orientation: 1, hasAlpha: true });
    expect(probeImage('png', fakePng(640, 480, 2)).hasAlpha).toBe(false);
  });
  it('WebP: size from VP8X', () => {
    expect(probeImage('webp', fakeWebp(1000, 700))).toMatchObject({ width: 1000, height: 700 });
  });
  it('JPEG: size from SOF, swapped for EXIF orientations 5–8', () => {
    expect(probeImage('jpeg', fakeJpeg(400, 300))).toMatchObject({ width: 400, height: 300, orientation: 1 });
    expect(probeImage('jpeg', fakeJpeg(400, 300, { orientation: 6 }))).toMatchObject({ width: 300, height: 400, orientation: 6 });
    expect(probeImage('jpeg', fakeJpeg(400, 300, { orientation: 3 }))).toMatchObject({ width: 400, height: 300, orientation: 3 });
    expect(jpegOrientation(fakeJpeg(10, 10, { orientation: 8 }))).toBe(8);
  });
  it('throws Corrupt on a header that ends early', () => {
    try { probeImage('png', fakePng(1, 1).subarray(0, 12)); expect.unreachable(); } catch (e) { expect((e as RefractError).code).toBe('Corrupt'); }
  });
});
