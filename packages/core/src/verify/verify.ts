import { ascii, detect, FORMATS } from '../detect/detect.ts';
import type { Inspect } from '../engine.ts';
import { RefractError } from '../errors.ts';
import type { Item } from '../graph/graph.ts';
import type { Expect } from '../tools/tools.ts';

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
    if (len < 2) break;
    if (marker === 0xe1 && p + 10 <= b.length && ascii(b, p + 4, p + 10) === 'Exif\0\0') hasExif = true;
    const isSof = marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc;
    if (isSof && p + 9 <= b.length) { height = u16(b, p + 5); width = u16(b, p + 7); }
    p += 2 + len;
  }
  const complete = b.length >= 4 && b[b.length - 2] === 0xff && b[b.length - 1] === 0xd9;
  return { width, height, hasExif, complete };
}

/** Allowed difference in seconds between expected and measured audio length (encoder padding). */
export const DURATION_TOLERANCE_S = 0.2;

const failed = (what: string) =>
  new RefractError('Internal', `the converted file didn't pass the final check (${what})`, 'Try again. If it keeps happening, please report it.');

/** Checks that need no decoder: size, type by magic bytes, JPEG completeness and EXIF. */
function checkBytes(out: Item, exp: Expect): void {
  if (out.bytes.length === 0) throw failed('it is empty');
  const format = detect(out.bytes);
  if (format !== exp.format) throw failed(`it is ${format === 'unknown' ? 'not a known type' : FORMATS[format].label}, not ${FORMATS[exp.format].label}`);
  if (format === 'jpeg') {
    const info = readJpegInfo(out.bytes);
    if (!info.complete) throw failed('it ends early');
    if (info.hasExif) throw failed('it still carries EXIF data');
  }
  if (exp.maxBytes !== undefined && out.bytes.length > exp.maxBytes) {
    throw failed(`it is ${Math.ceil(out.bytes.length / 1024)} KB, over the ${Math.round(exp.maxBytes / 1024)} KB target`);
  }
}

function checkInspect(got: Inspect, exp: Expect): void {
  if (exp.width !== undefined && (got.width !== exp.width || got.height !== exp.height)) {
    throw failed(`it is ${got.width}×${got.height}, expected ${exp.width}×${exp.height}`);
  }
  if (exp.pages !== undefined && got.pages !== exp.pages) throw failed(`it has ${got.pages} pages, expected ${exp.pages}`);
  if (exp.durationS !== undefined) {
    if (got.durationS === undefined || Math.abs(got.durationS - exp.durationS) > DURATION_TOLERANCE_S) {
      throw failed(`it is ${got.durationS?.toFixed(2)} s long, expected ${exp.durationS.toFixed(2)} s`);
    }
  }
}

/**
 * Verify every output before it is handed over: the count, each file's type and byte checks, then a
 * real decode (via the engine) for size, pages or duration. Any failed check throws, and the job ends Failed.
 */
export async function verifyOutputs(outs: Item[], expect: Expect[], inspect: (items: Item[]) => Promise<Inspect[]>): Promise<void> {
  if (outs.length !== expect.length) throw failed(`${outs.length} files came out, expected ${expect.length}`);
  outs.forEach((o, i) => checkBytes(o, expect[i]!));
  const needDecode = expect.some((e) => e.width !== undefined || e.pages !== undefined || e.durationS !== undefined);
  // Decode everything that has an expectation; an output that can't be reopened fails here too.
  const got = await inspect(outs);
  got.forEach((g, i) => {
    if (g.format === 'unknown') throw failed(`file ${i + 1} could not be opened again`);
  });
  if (needDecode) got.forEach((g, i) => checkInspect(g, expect[i]!));
}
