import { detect, ascii, type Format } from '../detect/detect.ts';
import { RefractError } from '../errors.ts';

export type JpegInfo = { width: number; height: number; hasExif: boolean; complete: boolean };

const u16 = (b: Uint8Array, p: number) => (b[p]! << 8) | b[p + 1]!;

/** Read a JPEG's frame size and whether it carries an Exif segment, by walking its markers. */
export function readJpegInfo(b: Uint8Array): JpegInfo {
  let width = 0;
  let height = 0;
  let hasExif = false;
  let p = 2;
  while (p + 4 <= b.length) {
    if (b[p] !== 0xff) break;
    const marker = b[p + 1]!;
    if (marker === 0xff) { p++; continue; }
    if (marker === 0x01 || (marker >= 0xd0 && marker <= 0xd8)) { p += 2; continue; }
    if (marker === 0xda) break; // start of scan: headers are done
    const len = u16(b, p + 2);
    if (marker === 0xe1 && ascii(b, p + 4, p + 10) === 'Exif\0\0') hasExif = true;
    const isSof = marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc;
    if (isSof) { height = u16(b, p + 5); width = u16(b, p + 7); }
    p += 2 + len;
  }
  const complete = b.length >= 4 && b[b.length - 2] === 0xff && b[b.length - 1] === 0xd9;
  return { width, height, hasExif, complete };
}

export type Expected = { format: Format; width: number; height: number };
/** Re-decodes the output with a real decoder and returns its size. Supplied by the engine. */
export type Decode = (bytes: Uint8Array, format: Format) => Promise<{ width: number; height: number }>;

const failed = (what: string) =>
  new RefractError('Internal', `the converted file didn't pass the final check (${what})`, 'Try again. If it keeps happening, please report it.');

/** Verify an output before it is handed over. Any failed check throws, and the job ends Failed. */
export async function verifyOutput(out: Uint8Array, expected: Expected, decode: Decode): Promise<void> {
  if (out.length === 0) throw failed('it is empty');
  const format = detect(out);
  if (format !== expected.format) throw failed(`it is ${format}, not ${expected.format}`);
  if (format === 'jpeg') {
    const info = readJpegInfo(out);
    if (!info.complete) throw failed('it ends early');
    if (info.hasExif) throw failed('it still carries EXIF data');
  }
  const decoded = await decode(out, format);
  if (decoded.width !== expected.width || decoded.height !== expected.height) {
    throw failed(`it is ${decoded.width}×${decoded.height}, expected ${expected.width}×${expected.height}`);
  }
}
