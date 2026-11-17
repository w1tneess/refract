import { jpegOrientation, RefractError, searchTargetSize, type Format, type Inspect, type Item, type Options } from '@refract/core';
import type { Ctx } from '../protocol.ts';

type Pixels = ImageData;

const damaged = (details?: string) =>
  new RefractError('Corrupt', "the file is damaged (the image data ends early or can't be decoded)", 'Try re-downloading it or exporting it again.', details);

async function timed<T>(ctx: Ctx, key: string, fn: () => Promise<T>): Promise<T> {
  const t = performance.now();
  const v = await fn();
  ctx.timing(key, performance.now() - t);
  ctx.heartbeat();
  return v;
}

async function decodeHeic(bytes: Uint8Array, ctx: Ctx): Promise<Pixels> {
  const { default: factory } = await timed(ctx, 'wasmLoadMs', () => import('libheif-js/libheif-wasm/libheif-bundle.mjs'));
  const libheif = factory();
  let images;
  try {
    images = new libheif.HeifDecoder().decode(bytes);
  } catch (e) {
    throw damaged(String(e));
  }
  const image = images.find((i) => i.is_primary()) ?? images[0];
  if (!image) throw damaged(ctx.logs.join('\n') || 'no images in file');
  // libheif applies the container's irot/imir, so width/height and pixels are already upright.
  const width = image.get_width();
  const height = image.get_height();
  const data = await new Promise<Uint8ClampedArray<ArrayBuffer> | null>((resolve) =>
    image.display({ data: new Uint8ClampedArray(width * height * 4), width, height }, (r) => resolve(r?.data ?? null)),
  );
  images.forEach((i) => i.free());
  if (!data) throw damaged(ctx.logs.join('\n') || 'decode returned no pixels');
  return new ImageData(data, width, height);
}

/**
 * Apply an EXIF orientation (1–8) to the pixels. The output carries no EXIF, so it must be upright.
 * Each case maps a source pixel (x, y) in a w×h image to its place in the result.
 */
export function orient(img: Pixels, o: number): Pixels {
  if (o <= 1 || o > 8) return img;
  const { width: w, height: h } = img;
  const swap = o >= 5;
  const W = swap ? h : w;
  const H = swap ? w : h;
  const src = new Uint32Array(img.data.buffer, img.data.byteOffset, w * h);
  const out = new Uint8ClampedArray(W * H * 4);
  const dst = new Uint32Array(out.buffer);
  const at: Record<number, (x: number, y: number) => number> = {
    2: (x, y) => y * W + (w - 1 - x),
    3: (x, y) => (h - 1 - y) * W + (w - 1 - x),
    4: (x, y) => (h - 1 - y) * W + x,
    5: (x, y) => x * W + y,
    6: (x, y) => x * W + (h - 1 - y),
    7: (x, y) => (w - 1 - x) * W + (h - 1 - y),
    8: (x, y) => (w - 1 - x) * W + y,
  };
  const map = at[o]!;
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) dst[map(x, y)] = src[y * w + x]!;
  return new ImageData(out, W, H);
}

/** Decode any supported image to upright RGBA. */
export async function decode(item: Item, ctx: Ctx): Promise<Pixels> {
  const t = performance.now();
  let img: Pixels;
  try {
    switch (item.format) {
      case 'heic':
        img = await decodeHeic(item.bytes, ctx);
        break;
      case 'jpeg': {
        const { default: dec } = await import('@jsquash/jpeg/decode');
        img = orient(await dec(item.bytes.slice().buffer as ArrayBuffer), jpegOrientation(item.bytes));
        break;
      }
      case 'png': {
        const { default: dec } = await import('@jsquash/png/decode');
        img = await dec(item.bytes.slice().buffer as ArrayBuffer);
        break;
      }
      case 'webp': {
        const { default: dec } = await import('@jsquash/webp/decode');
        img = await dec(item.bytes.slice().buffer as ArrayBuffer);
        break;
      }
      default:
        throw new RefractError('Unsupported', `${item.format} is not an image`, 'Pick an image.');
    }
  } catch (e) {
    if (e instanceof RefractError) throw e;
    throw damaged(e instanceof Error ? e.message : String(e));
  }
  ctx.timing('decodeMs', performance.now() - t);
  ctx.heartbeat();
  return img;
}

/** JPEG has no transparency: put transparent pixels on white instead of black. */
function flatten(img: Pixels): Pixels {
  const d = new Uint8ClampedArray(img.data);
  for (let i = 0; i < d.length; i += 4) {
    const a = d[i + 3]! / 255;
    if (a === 1) continue;
    d[i] = d[i]! * a + 255 * (1 - a);
    d[i + 1] = d[i + 1]! * a + 255 * (1 - a);
    d[i + 2] = d[i + 2]! * a + 255 * (1 - a);
    d[i + 3] = 255;
  }
  return new ImageData(d, img.width, img.height);
}

/** Encode RGBA. mozjpeg, libwebp and the PNG encoder write no EXIF, so no orientation tag survives. */
export async function encode(img: Pixels, to: Format, quality: number, ctx: Ctx): Promise<Uint8Array> {
  const t = performance.now();
  let out: ArrayBuffer;
  if (to === 'jpeg') {
    const { default: enc } = await import('@jsquash/jpeg/encode');
    out = await enc(flatten(img), { quality });
  } else if (to === 'webp') {
    const { default: enc } = await import('@jsquash/webp/encode');
    out = await enc(img, { quality });
  } else if (to === 'png') {
    const { default: enc } = await import('@jsquash/png/encode');
    out = await enc(img);
  } else {
    throw new RefractError('Unsupported', `Refract can't write ${to} images`, 'Pick JPG, PNG or WebP.');
  }
  ctx.timing('encodeMs', performance.now() - t);
  ctx.heartbeat();
  return new Uint8Array(out);
}

export async function resize(img: Pixels, width: number, height: number, ctx: Ctx): Promise<Pixels> {
  if (img.width === width && img.height === height) return img;
  const { default: rs } = await import('@jsquash/resize');
  const t = performance.now();
  const out = await rs(img, { width, height, method: 'lanczos3', premultiply: true, linearRGB: true });
  ctx.timing('resizeMs', performance.now() - t);
  ctx.heartbeat();
  return out;
}

export async function run(op: string, items: Item[], opts: Options, ctx: Ctx): Promise<Item[]> {
  const [item] = items;
  if (!item) throw new RefractError('Internal', 'no input', '');
  const to = opts.to as Format;
  if (op === 'image.convert') {
    let img = await decode(item, ctx);
    if (opts.width && opts.height) img = await resize(img, Number(opts.width), Number(opts.height), ctx);
    return [{ bytes: await encode(img, to, Number(opts.quality ?? 90), ctx), format: to }];
  }
  if (op === 'image.target') {
    const img = await decode(item, ctx);
    const result = await searchTargetSize(
      async (q, scale) => {
        const sized = scale < 1 ? await resize(img, Math.max(1, Math.round(img.width * scale)), Math.max(1, Math.round(img.height * scale)), ctx) : img;
        return encode(sized, to, q, ctx);
      },
      Number(opts.targetBytes),
      ctx.heartbeat,
    );
    return [{ bytes: result.bytes, format: to }];
  }
  throw new RefractError('Internal', `unknown operation ${op}`, '');
}

/** Reopen outputs with the browser's own decoder (independent of the encoders above). */
export async function inspect(item: Item): Promise<Inspect> {
  try {
    const bitmap = await createImageBitmap(new Blob([item.bytes as BlobPart], { type: `image/${item.format}` }), { imageOrientation: 'none' });
    const info: Inspect = { format: item.format, width: bitmap.width, height: bitmap.height };
    bitmap.close();
    return info;
  } catch {
    return { format: 'unknown' };
  }
}
