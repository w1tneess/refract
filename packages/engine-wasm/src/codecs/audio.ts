import { ext, RefractError, type Format, type Inspect, type Item, type Options } from '@refract/core';
import type { FFmpeg } from '@ffmpeg/ffmpeg';
import type { Ctx } from '../protocol.ts';

const damaged = (details?: string) =>
  new RefractError('Corrupt', "the file is damaged and can't be decoded", 'Try re-downloading it or exporting it again.', details);

let ffmpegPromise: Promise<FFmpeg> | null = null;

async function resolveWasmUrl(url: string): Promise<{ url: string; revoke: boolean }> {
  if (!url.endsWith('.gz')) return { url, revoke: false };
  const resolved = new URL(url, self.location.href).href;
  const res = await fetch(resolved);
  if (!res.ok) throw new Error(`Failed to fetch FFmpeg WASM (${res.status})`);
  const raw = new Uint8Array(await res.arrayBuffer());
  let wasmBytes: ArrayBuffer;
  // Detect gzip magic bytes (0x1f 0x8b); if the host already decompressed it, use raw WASM bytes directly.
  if (raw.length >= 2 && raw[0] === 0x1f && raw[1] === 0x8b) {
    if (typeof DecompressionStream === 'undefined') {
      throw new Error('This browser cannot decompress the bundled FFmpeg WebAssembly module.');
    }
    const stream = new Blob([raw]).stream().pipeThrough(new DecompressionStream('gzip'));
    wasmBytes = await new Response(stream).arrayBuffer();
  } else {
    wasmBytes = raw.buffer.slice(raw.byteOffset, raw.byteOffset + raw.byteLength);
  }
  return { url: URL.createObjectURL(new Blob([wasmBytes], { type: 'application/wasm' })), revoke: true };
}

async function getFFmpeg(ctx: Ctx): Promise<FFmpeg> {
  if (ffmpegPromise) return ffmpegPromise;
  const t = performance.now();
  ffmpegPromise = (async () => {
    let wasmBlobUrl: string | null = null;
    try {
      const [{ FFmpeg }, { default: coreJsUrl }, { default: coreWasmUrl }] = await Promise.all([
        import('@ffmpeg/ffmpeg'),
        import('@ffmpeg/core?url'),
        import('@ffmpeg/core/wasm?url'),
      ]);
      const wasm = await resolveWasmUrl(coreWasmUrl);
      if (wasm.revoke) wasmBlobUrl = wasm.url;
      const ffmpeg = new FFmpeg();
      await ffmpeg.load({
        coreURL: coreJsUrl,
        wasmURL: wasm.url,
      });
      ctx.timing('wasmLoadMs', performance.now() - t);
      ctx.heartbeat();
      return ffmpeg;
    } catch (e) {
      ffmpegPromise = null;
      throw e;
    } finally {
      if (wasmBlobUrl) URL.revokeObjectURL(wasmBlobUrl);
    }
  })();
  return ffmpegPromise;
}

export async function inspect(item: Item, ctx: Ctx): Promise<Inspect> {
  const t = performance.now();
  let ffmpeg: FFmpeg | undefined;
  let inName: string | undefined;
  let durationS: number | undefined;
  const onLog = ({ message }: { message: string }) => {
    ctx.logs.push(message);
    const m = message.match(/Duration:\s*(\d{2}):(\d{2}):(\d{2}(?:\.\d+)?)/);
    if (m) {
      durationS = Number(m[1]) * 3600 + Number(m[2]) * 60 + Number(m[3]);
    }
  };
  try {
    ffmpeg = await getFFmpeg(ctx);
    inName = `inspect_${Date.now()}.${ext(item.format)}`;

    ffmpeg.on('log', onLog);
    await ffmpeg.writeFile(inName, item.bytes);
    // '-f null -' processes input and writes null format output, printing media stats
    await ffmpeg.exec(['-i', inName, '-f', 'null', '-']);

    ctx.timing('inspectMs', performance.now() - t);
    ctx.heartbeat();

    if (durationS === undefined) {
      return { format: 'unknown' };
    }
    return { format: item.format, durationS };
  } catch (e) {
    if (e instanceof RefractError) throw e;
    return { format: 'unknown' };
  } finally {
    if (ffmpeg) {
      ffmpeg.off('log', onLog);
      if (inName) {
        try { await ffmpeg.deleteFile(inName); } catch {}
      }
    }
  }
}

function codecArgs(to: Format, bitrate?: string | number): string[] {
  const b = String(bitrate ?? '192k');
  switch (to) {
    case 'mp3': return ['-c:a', 'libmp3lame', '-b:a', b];
    case 'wav': return ['-c:a', 'pcm_s16le'];
    case 'ogg': return ['-c:a', 'libvorbis', '-b:a', b];
    case 'm4a': return ['-c:a', 'aac', '-b:a', b];
    default:
      throw new RefractError('Unsupported', `Refract can't write ${to} audio`, 'Pick MP3, WAV, M4A or OGG.');
  }
}

export async function run(op: string, items: Item[], opts: Options, ctx: Ctx): Promise<Item[]> {
  const [item] = items;
  if (!item) throw new RefractError('Internal', 'no input', '');

  const ffmpeg = await getFFmpeg(ctx);
  const inExt = ext(item.format);
  const toFormat = (opts.to as Format) || item.format;
  const outExt = ext(toFormat);
  const inName = `input_${Date.now()}.${inExt}`;
  const outName = `output_${Date.now()}.${outExt}`;

  const onLog = ({ message }: { message: string }) => {
    ctx.logs.push(message);
    ctx.heartbeat();
  };
  ffmpeg.on('log', onLog);

  try {
    const t = performance.now();
    await ffmpeg.writeFile(inName, item.bytes);

    let args: string[];
    if (op === 'audio.convert') {
      args = ['-i', inName, ...codecArgs(toFormat, opts.bitrate), outName];
    } else if (op === 'audio.trim') {
      const start = Number(opts.start ?? 0);
      const end = Number(opts.end ?? 0);
      args = ['-ss', String(start)];
      if (end > start) args.push('-to', String(end));
      args.push('-i', inName, ...codecArgs(toFormat, opts.bitrate), outName);
    } else {
      throw new RefractError('Internal', `unknown audio operation ${op}`, '');
    }

    const code = await ffmpeg.exec(args);
    ctx.timing('execMs', performance.now() - t);
    ctx.heartbeat();

    if (code !== 0) {
      throw damaged(ctx.logs.slice(-10).join('\n') || `ffmpeg exited with code ${code}`);
    }

    const outData = await ffmpeg.readFile(outName);
    const bytes = typeof outData === 'string' ? new TextEncoder().encode(outData) : new Uint8Array(outData);

    return [{ bytes, format: toFormat }];
  } catch (e) {
    if (e instanceof RefractError) throw e;
    throw damaged(e instanceof Error ? e.message : String(e));
  } finally {
    try { await ffmpeg.deleteFile(inName); } catch {}
    try { await ffmpeg.deleteFile(outName); } catch {}
    ffmpeg.off('log', onLog);
  }
}
