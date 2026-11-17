import { RefractError } from '../errors.ts';
import { ascii, type Format } from './detect.ts';
import { probeHeif } from './heif.ts';

export type ImageProbe = {
  /** Displayed size, after orientation is applied. */
  width: number;
  height: number;
  /** EXIF orientation 1–8 for JPEG (applied by the engine, then stripped); 1 for other formats. */
  orientation: number;
  hasAlpha: boolean;
};

const corrupt = (why: string) =>
  new RefractError('Corrupt', `the file is damaged (${why})`, 'Try re-downloading it or exporting it again.');

const be16 = (b: Uint8Array, p: number) => (b[p]! << 8) | b[p + 1]!;
const be32 = (b: Uint8Array, p: number) => ((b[p]! << 24) >>> 0) + (b[p + 1]! << 16) + (b[p + 2]! << 8) + b[p + 3]!;
const le16 = (b: Uint8Array, p: number) => b[p]! | (b[p + 1]! << 8);
const le24 = (b: Uint8Array, p: number) => b[p]! | (b[p + 1]! << 8) | (b[p + 2]! << 16);

/** Orientations 5–8 swap width and height. */
export const swapsAxes = (o: number) => o >= 5 && o <= 8;

function need(b: Uint8Array, n: number, what: string) {
  if (b.length < n) throw corrupt(`${what} ends early`);
}

/** EXIF orientation from a JPEG APP1 segment, or 1 when absent. */
export function jpegOrientation(b: Uint8Array): number {
  let p = 2;
  while (p + 4 <= b.length && b[p] === 0xff) {
    const marker = b[p + 1]!;
    if (marker === 0xda || marker === 0xd9) break;
    const len = be16(b, p + 2);
    if (marker === 0xe1 && ascii(b, p + 4, p + 10) === 'Exif\0\0') {
      const t = p + 10; // TIFF header
      const little = ascii(b, t, t + 2) === 'II';
      const u16 = (q: number) => (little ? le16(b, q) : be16(b, q));
      const u32 = (q: number) => (little ? (b[q]! | (b[q + 1]! << 8) | (b[q + 2]! << 16)) + b[q + 3]! * 2 ** 24 : be32(b, q));
      if (t + 8 > b.length) return 1;
      const ifd = t + u32(t + 4);
      if (ifd + 2 > b.length) return 1;
      const n = u16(ifd);
      for (let i = 0; i < n; i++) {
        const e = ifd + 2 + i * 12;
        if (e + 12 > b.length) break;
        if (u16(e) === 0x0112) {
          const o = u16(e + 8);
          return o >= 1 && o <= 8 ? o : 1;
        }
      }
      return 1;
    }
    p += 2 + len;
  }
  return 1;
}

function probeJpeg(b: Uint8Array): ImageProbe {
  let p = 2;
  while (p + 9 <= b.length) {
    if (b[p] !== 0xff) throw corrupt('a header is malformed');
    const marker = b[p + 1]!;
    if (marker === 0xff) { p++; continue; }
    if (marker === 0xd8 || marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) { p += 2; continue; }
    if (marker === 0xda || marker === 0xd9) break;
    const sof = marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc;
    if (sof) {
      const h = be16(b, p + 5);
      const w = be16(b, p + 7);
      if (!w || !h) throw corrupt('the image size is zero');
      const orientation = jpegOrientation(b);
      return swapsAxes(orientation)
        ? { width: h, height: w, orientation, hasAlpha: false }
        : { width: w, height: h, orientation, hasAlpha: false };
    }
    p += 2 + be16(b, p + 2);
  }
  throw corrupt('the image size is missing');
}

function probePng(b: Uint8Array): ImageProbe {
  need(b, 26, 'the header');
  if (ascii(b, 12, 16) !== 'IHDR') throw corrupt('the header is malformed');
  const width = be32(b, 16);
  const height = be32(b, 20);
  if (!width || !height) throw corrupt('the image size is zero');
  const colorType = b[25]!;
  const hasAlpha = colorType === 4 || colorType === 6 || ascii(b, 0, Math.min(b.length, 4096)).includes('tRNS');
  return { width, height, orientation: 1, hasAlpha };
}

function probeWebp(b: Uint8Array): ImageProbe {
  need(b, 30, 'the header');
  const chunk = ascii(b, 12, 16);
  if (chunk === 'VP8X') {
    return { width: le24(b, 24) + 1, height: le24(b, 27) + 1, orientation: 1, hasAlpha: (b[20]! & 0x10) !== 0 };
  }
  if (chunk === 'VP8 ') {
    return { width: le16(b, 26) & 0x3fff, height: le16(b, 28) & 0x3fff, orientation: 1, hasAlpha: false };
  }
  if (chunk === 'VP8L') {
    const bits = b[21]! | (b[22]! << 8) | (b[23]! << 16) | (b[24]! << 24);
    return { width: (bits & 0x3fff) + 1, height: ((bits >>> 14) & 0x3fff) + 1, orientation: 1, hasAlpha: ((bits >>> 28) & 1) === 1 };
  }
  throw corrupt('the header is malformed');
}

/** Read an image's displayed size and orientation from its header, without decoding pixels. */
export function probeImage(format: Format, b: Uint8Array): ImageProbe {
  switch (format) {
    case 'heic': {
      const p = probeHeif(b);
      return { width: p.width, height: p.height, orientation: 1, hasAlpha: false };
    }
    case 'jpeg': return probeJpeg(b);
    case 'png': return probePng(b);
    case 'webp': return probeWebp(b);
    default: throw new Error(`probeImage: ${format} is not an image`);
  }
}
