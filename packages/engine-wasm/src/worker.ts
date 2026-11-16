/// <reference lib="webworker" />
import type { WorkerRequest, WorkerResponse } from './protocol.ts';

declare const self: DedicatedWorkerGlobalScope;

const post = (m: WorkerResponse, transfer: Transferable[] = []) => self.postMessage(m, transfer);

// libheif-js logs decode failures to console instead of throwing; keep them for the "Details" toggle.
const logs: string[] = [];
for (const level of ['log', 'warn', 'error'] as const) {
  const original = console[level].bind(console);
  console[level] = (...args: unknown[]) => {
    logs.push(args.map(String).join(' '));
    original(...args);
  };
}

const damaged = (details?: string): WorkerResponse => ({
  type: 'error',
  code: 'Corrupt',
  why: "the file is damaged (the image data ends early or can't be decoded)",
  todo: 'Try re-downloading it or exporting it from your phone again.',
  details,
});

async function convert(bytes: Uint8Array, quality: number) {
  const tLoad = performance.now();
  const [{ default: libheifFactory }, { default: encode }] = await Promise.all([
    import('libheif-js/libheif-wasm/libheif-bundle.mjs'),
    import('@jsquash/jpeg/encode'),
  ]);
  const libheif = libheifFactory();
  post({ type: 'timing', key: 'wasmLoadMs', ms: performance.now() - tLoad });
  post({ type: 'heartbeat' });

  const tDecode = performance.now();
  let images;
  try {
    images = new libheif.HeifDecoder().decode(bytes);
  } catch (e) {
    return post(damaged(String(e)));
  }
  const image = images.find((i) => i.is_primary()) ?? images[0];
  if (!image) return post(damaged(logs.join('\n') || 'no images in file'));
  // libheif applies the container's irot/imir, so width/height and pixels are already upright.
  const width = image.get_width();
  const height = image.get_height();
  const pixels = await new Promise<Uint8ClampedArray<ArrayBuffer> | null>((resolve) =>
    image.display({ data: new Uint8ClampedArray(width * height * 4), width, height }, (r) => resolve(r?.data ?? null)),
  );
  images.forEach((i) => i.free());
  if (!pixels) return post(damaged(logs.join('\n') || 'decode returned no pixels'));
  post({ type: 'timing', key: 'decodeMs', ms: performance.now() - tDecode });
  post({ type: 'heartbeat' });

  const tEncode = performance.now();
  // mozjpeg writes JFIF only, so no EXIF (and no orientation tag) reaches the output.
  const jpeg = new Uint8Array(await encode(new ImageData(pixels, width, height), { quality }));
  post({ type: 'timing', key: 'encodeMs', ms: performance.now() - tEncode });
  post({ type: 'done', bytes: jpeg }, [jpeg.buffer]);
}

async function inspect(bytes: Uint8Array, mime: string) {
  // The browser's own decoder, independent of the encoder we used. 'none' = report stored size.
  const bitmap = await createImageBitmap(new Blob([bytes as BlobPart], { type: mime }), { imageOrientation: 'none' });
  post({ type: 'done', width: bitmap.width, height: bitmap.height });
  bitmap.close();
}

self.onmessage = async (e: MessageEvent<WorkerRequest>) => {
  const msg = e.data;
  post({ type: 'heartbeat' });
  try {
    if (msg.type === 'convert') await convert(msg.bytes, msg.quality);
    else await inspect(msg.bytes, msg.mime);
  } catch (err) {
    post({
      type: 'error',
      code: 'Internal',
      why: 'something went wrong inside the converter',
      todo: 'Try again. If it keeps happening, please report it.',
      details: [err instanceof Error ? `${err.name}: ${err.message}` : String(err), ...logs].join('\n'),
    });
  }
};
