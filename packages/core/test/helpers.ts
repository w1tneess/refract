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

/** Smallest byte string readJpegInfo accepts: SOI, optional APP1 Exif, SOF0 with a size, EOI. */
export function fakeJpeg(width: number, height: number, opts: { exif?: boolean; truncated?: boolean } = {}): Uint8Array {
  const exif = opts.exif ? [0xff, 0xe1, 0x00, 0x08, 0x45, 0x78, 0x69, 0x66, 0x00, 0x00] : [];
  const sof = [0xff, 0xc0, 0x00, 0x0b, 0x08, height >> 8, height & 255, width >> 8, width & 255, 0x01, 0x01, 0x11, 0x00];
  const eoi = opts.truncated ? [] : [0xff, 0xd9];
  return new Uint8Array([0xff, 0xd8, ...exif, ...sof, 0xff, 0xda, 0x00, 0x02, ...eoi]);
}
