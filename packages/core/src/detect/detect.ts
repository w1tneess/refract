export type Format = 'heic' | 'jpeg';

// shortcut: phase-1 brands only. `mif1` is also used by AVIF; once AVIF is in scope, check compatible brands too.
const HEIC_BRANDS = new Set(['heic', 'heix', 'mif1']);

const EXTENSIONS: Record<Format, string[]> = {
  heic: ['heic', 'heif'],
  jpeg: ['jpg', 'jpeg', 'jfif'],
};

const LABEL: Record<Format, string> = { heic: 'HEIC', jpeg: 'JPEG' };

export function ascii(b: Uint8Array, start: number, end: number): string {
  return String.fromCharCode(...b.subarray(start, end));
}

/** Identify a file by its first bytes. The extension is never consulted. */
export function detect(b: Uint8Array): Format | 'unknown' {
  if (b.length >= 3 && b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) return 'jpeg';
  if (b.length >= 12 && ascii(b, 4, 8) === 'ftyp' && HEIC_BRANDS.has(ascii(b, 8, 12))) return 'heic';
  return 'unknown';
}

/** A notice when the file name's extension disagrees with the bytes, or null when it agrees. */
export function extensionMismatch(fileName: string, format: Format): string | null {
  const dot = fileName.lastIndexOf('.');
  const ext = dot > 0 ? fileName.slice(dot + 1).toLowerCase() : '';
  if (EXTENSIONS[format].includes(ext)) return null;
  const named = ext ? `named .${ext}` : 'has no extension';
  return `${fileName} ${named}, but it is really a ${LABEL[format]} image. Converting it as ${LABEL[format]}.`;
}
