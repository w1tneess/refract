import type { Clock } from '../src/jobs/run.ts';

/** A fake clock: time moves only when the test calls advance(). */
export function fakeClock() {
  let now = 0;
  const timers = new Set<{ ms: number; next: number; fn: () => void }>();
  const clock: Clock = {
    now: () => now,
    every(ms, fn) {
      const t = { ms, next: now + ms, fn };
      timers.add(t);
      return () => void timers.delete(t);
    },
  };
  async function advance(ms: number) {
    const end = now + ms;
    while (now < end) {
      now = Math.min(end, now + 250);
      for (const t of [...timers]) while (t.next <= now && timers.has(t)) { t.next += t.ms; t.fn(); }
      for (let i = 0; i < 20; i++) await Promise.resolve(); // microtasks only: setTimeout(0) is ~15 ms on Windows
    }
  }
  return { clock, advance };
}

export const flush = () => new Promise<void>((r) => setTimeout(r, 0));

const be32 = (n: number) => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];

/** Smallest byte string readJpegInfo accepts: SOI, optional APP1 Exif (with orientation), SOF0 with a size, EOI. */
export function fakeJpeg(width: number, height: number, opts: { exif?: boolean; orientation?: number; truncated?: boolean } = {}): Uint8Array {
  let app1: number[] = [];
  if (opts.orientation) {
    // Big-endian TIFF, one IFD entry: 0x0112 SHORT 1 = orientation.
    const tiff = [0x4d, 0x4d, 0, 42, 0, 0, 0, 8, 0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, opts.orientation, 0, 0, 0, 0, 0, 0];
    const body = [...[0x45, 0x78, 0x69, 0x66, 0, 0], ...tiff];
    app1 = [0xff, 0xe1, (body.length + 2) >> 8, (body.length + 2) & 255, ...body];
  } else if (opts.exif) {
    app1 = [0xff, 0xe1, 0x00, 0x08, 0x45, 0x78, 0x69, 0x66, 0x00, 0x00];
  }
  const sof = [0xff, 0xc0, 0x00, 0x0b, 0x08, height >> 8, height & 255, width >> 8, width & 255, 0x01, 0x01, 0x11, 0x00];
  const eoi = opts.truncated ? [] : [0xff, 0xd9];
  return new Uint8Array([0xff, 0xd8, ...app1, ...sof, 0xff, 0xda, 0x00, 0x02, ...eoi]);
}

export function fakePng(width: number, height: number, colorType = 6): Uint8Array {
  return new Uint8Array([0x89, 0x50, 0x4e, 0x47, 13, 10, 26, 10, 0, 0, 0, 13, ...[73, 72, 68, 82], ...be32(width), ...be32(height), 8, colorType, 0, 0, 0, 0, 0, 0, 0]);
}

export function fakeWebp(width: number, height: number): Uint8Array {
  const w = width - 1;
  const h = height - 1;
  const ascii = (s: string) => [...s].map((c) => c.charCodeAt(0));
  return new Uint8Array([...ascii('RIFF'), 30, 0, 0, 0, ...ascii('WEBP'), ...ascii('VP8X'), 10, 0, 0, 0, 0x10, 0, 0, 0, w & 255, (w >> 8) & 255, w >> 16, h & 255, (h >> 8) & 255, h >> 16]);
}

export const fakePdf = (tag = '') => new TextEncoder().encode(`%PDF-1.7\n%${tag}\n`);
export const fakeMp3 = () => new Uint8Array([0x49, 0x44, 0x33, 4, 0, 0, 0, 0, 0, 0]);
