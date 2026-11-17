export type Format = 'heic' | 'jpeg' | 'png' | 'webp' | 'pdf' | 'mp3' | 'wav' | 'm4a' | 'ogg';
export type Kind = 'image' | 'pdf' | 'audio';

export const FORMATS: Record<Format, { label: string; ext: string[]; mime: string; kind: Kind }> = {
  heic: { label: 'HEIC', ext: ['heic', 'heif'], mime: 'image/heic', kind: 'image' },
  jpeg: { label: 'JPG', ext: ['jpg', 'jpeg', 'jfif'], mime: 'image/jpeg', kind: 'image' },
  png: { label: 'PNG', ext: ['png'], mime: 'image/png', kind: 'image' },
  webp: { label: 'WebP', ext: ['webp'], mime: 'image/webp', kind: 'image' },
  pdf: { label: 'PDF', ext: ['pdf'], mime: 'application/pdf', kind: 'pdf' },
  mp3: { label: 'MP3', ext: ['mp3'], mime: 'audio/mpeg', kind: 'audio' },
  wav: { label: 'WAV', ext: ['wav'], mime: 'audio/wav', kind: 'audio' },
  m4a: { label: 'M4A', ext: ['m4a', 'aac', 'mp4'], mime: 'audio/mp4', kind: 'audio' },
  ogg: { label: 'OGG', ext: ['ogg', 'oga'], mime: 'audio/ogg', kind: 'audio' },
};

export const ext = (f: Format) => FORMATS[f].ext[0]!;
export const formatsOfKind = (k: Kind) => (Object.keys(FORMATS) as Format[]).filter((f) => FORMATS[f].kind === k);

// shortcut: `mif1` is also used by AVIF; AVIF is out of v1 scope, so a mif1 file is treated as HEIC.
const HEIC_BRANDS = new Set(['heic', 'heix', 'heim', 'heis', 'mif1', 'msf1']);
const M4A_BRANDS = new Set(['M4A ', 'M4B ', 'mp42', 'isom', 'iso2', 'dash']);

declare const TextDecoder: {
  new (label?: string, options?: { fatal?: boolean; ignoreBOM?: boolean }): {
    decode(input?: Uint8Array | ArrayBufferView | ArrayBuffer, options?: { stream?: boolean }): string;
  };
};

const asciiDecoder = new TextDecoder();

export function ascii(b: Uint8Array, start: number, end: number): string {
  return asciiDecoder.decode(b.subarray(start, end));
}

/** Identify a file by its first bytes. The extension is never consulted. */
export function detect(b: Uint8Array): Format | 'unknown' {
  if (b.length < 4) return 'unknown';
  if (b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) return 'jpeg';
  if (b[0] === 0x89 && ascii(b, 1, 4) === 'PNG') return 'png';
  if (ascii(b, 0, 4) === 'RIFF' && b.length >= 12) {
    if (ascii(b, 8, 12) === 'WEBP') return 'webp';
    if (ascii(b, 8, 12) === 'WAVE') return 'wav';
  }
  if (b.length >= 12 && ascii(b, 4, 8) === 'ftyp') {
    const brand = ascii(b, 8, 12);
    if (HEIC_BRANDS.has(brand)) return 'heic';
    if (M4A_BRANDS.has(brand)) return 'm4a';
  }
  if (ascii(b, 0, 4) === 'OggS') return 'ogg';
  // %PDF- may follow a little junk; readers accept it within the first 1 KB.
  if (ascii(b, 0, Math.min(b.length, 1024)).includes('%PDF-')) return 'pdf';
  if (ascii(b, 0, 3) === 'ID3') return 'mp3';
  // MPEG audio frame sync, layer bits non-zero (zero would be ADTS AAC).
  if (b[0] === 0xff && (b[1]! & 0xe0) === 0xe0 && ((b[1]! >> 1) & 3) !== 0) return 'mp3';
  return 'unknown';
}

/** Best guess from a file name, used only to pre-assign output names; detection still decides. */
export function formatFromName(name: string): Format | undefined {
  const e = name.slice(name.lastIndexOf('.') + 1).toLowerCase();
  return (Object.keys(FORMATS) as Format[]).find((f) => FORMATS[f].ext.includes(e));
}

/** A notice when the file name's extension disagrees with the bytes, or null when it agrees. */
export function extensionMismatch(fileName: string, format: Format): string | null {
  const dot = fileName.lastIndexOf('.');
  const e = dot > 0 ? fileName.slice(dot + 1).toLowerCase() : '';
  if (FORMATS[format].ext.includes(e)) return null;
  const named = e ? `named .${e}` : 'has no extension';
  const what = FORMATS[format].kind === 'image' ? 'image' : FORMATS[format].kind === 'pdf' ? 'document' : 'audio file';
  return `${fileName} ${named}, but it is really a ${FORMATS[format].label} ${what}. Converting it as ${FORMATS[format].label}.`;
}
