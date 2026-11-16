import { RefractError } from '../errors.ts';
import { ascii } from './detect.ts';

export type HeifProbe = {
  /** Displayed size, after the container's rotation is applied. */
  width: number;
  height: number;
  /** Anti-clockwise rotation from the `irot` box, in degrees. */
  rotation: 0 | 90 | 180 | 270;
  mirrored: boolean;
};

// HEIF stores orientation in irot/imir, which decoders apply. EXIF orientation inside a HEIC is
// informational only and must be ignored, or the photo gets rotated twice.

const corrupt = (why: string) =>
  new RefractError('Corrupt', `the file is damaged (${why})`, 'Try re-downloading it or exporting it from your phone again.');

const u16 = (b: Uint8Array, p: number) => (b[p]! << 8) | b[p + 1]!;
const u32 = (b: Uint8Array, p: number) => ((b[p]! << 24) >>> 0) + (b[p + 1]! << 16) + (b[p + 2]! << 8) + b[p + 3]!;

type Box = { type: string; start: number; end: number };

function* boxes(b: Uint8Array, start: number, end: number): Generator<Box> {
  let p = start;
  while (p + 8 <= end) {
    let size = u32(b, p);
    const type = ascii(b, p + 4, p + 8);
    let header = 8;
    if (size === 1) {
      if (p + 16 > end) throw corrupt(`the ${type} box ends early`);
      size = u32(b, p + 8) * 2 ** 32 + u32(b, p + 12);
      header = 16;
    } else if (size === 0) {
      size = end - p;
    }
    if (size < header || p + size > end) throw corrupt(`the ${type} box ends early`);
    yield { type, start: p + header, end: p + size };
    p += size;
  }
}

const child = (b: Uint8Array, parent: Box, type: string, offset = 0) => {
  for (const box of boxes(b, parent.start + offset, parent.end)) if (box.type === type) return box;
  return undefined;
};

/** Read the primary image's displayed size and rotation from the HEIF container, without decoding pixels. */
export function probeHeif(b: Uint8Array): HeifProbe {
  let meta: Box | undefined;
  // Walk top-level boxes lazily; stop at meta so a truncated mdat after it doesn't matter here.
  for (const box of boxes(b, 0, b.length)) if (box.type === 'meta') { meta = box; break; }
  if (!meta) throw corrupt('it has no image metadata');

  const pitm = child(b, meta, 'pitm', 4);
  const iprp = child(b, meta, 'iprp', 4);
  const ipco = iprp && child(b, iprp, 'ipco');
  const ipma = iprp && child(b, iprp, 'ipma');
  if (!pitm || !ipco || !ipma) throw corrupt('the image metadata is incomplete');

  const primary = b[pitm.start] === 0 ? u16(b, pitm.start + 4) : u32(b, pitm.start + 4);
  const props = [...boxes(b, ipco.start, ipco.end)];

  const version = b[ipma.start]!;
  const wideIndex = (b[ipma.start + 3]! & 1) === 1;
  let p = ipma.start + 4;
  const count = u32(b, p);
  p += 4;
  let assoc: Box[] = [];
  for (let i = 0; i < count && p < ipma.end; i++) {
    const item = version < 1 ? u16(b, p) : u32(b, p);
    p += version < 1 ? 2 : 4;
    const n = b[p++]!;
    const found: Box[] = [];
    for (let j = 0; j < n; j++) {
      const index = wideIndex ? u16(b, p) & 0x7fff : b[p]! & 0x7f;
      p += wideIndex ? 2 : 1;
      const prop = props[index - 1];
      if (prop) found.push(prop);
    }
    if (item === primary) assoc = found;
  }

  const ispe = assoc.find((x) => x.type === 'ispe');
  if (!ispe || ispe.end - ispe.start < 12) throw corrupt('the image size is missing');
  const w = u32(b, ispe.start + 4);
  const h = u32(b, ispe.start + 8);
  if (w === 0 || h === 0) throw corrupt('the image size is zero');

  const irot = assoc.find((x) => x.type === 'irot');
  const rotation = (((irot ? b[irot.start]! & 3 : 0) * 90) as HeifProbe['rotation']);
  const swap = rotation === 90 || rotation === 270;
  return {
    width: swap ? h : w,
    height: swap ? w : h,
    rotation,
    mirrored: assoc.some((x) => x.type === 'imir'),
  };
}
