import { jpegOrientation, RefractError, type Format, type Inspect, type Item, type Options } from '@refract/core';
import type { Ctx } from '../protocol.ts';
import { decode, encode } from './image.ts';

const damaged = (details?: string) =>
  new RefractError('Corrupt', "the file is damaged and can't be opened", 'Try re-downloading it or exporting it again.', details);

async function getQpdf() {
  const [{ default: factory }, { default: wasmUrl }] = await Promise.all([
    import('@neslinesli93/qpdf-wasm'),
    import('@neslinesli93/qpdf-wasm/dist/qpdf.wasm?url'),
  ]);
  // Fresh instance per job: Emscripten callMain is single-use per instance.
  return factory({ locateFile: () => wasmUrl });
}

export async function inspect(item: Item, ctx: Ctx): Promise<Inspect> {
  const t = performance.now();
  try {
    const { PDFDocument } = await import('pdf-lib');
    const doc = await PDFDocument.load(item.bytes, { ignoreEncryption: true });
    ctx.timing('inspectMs', performance.now() - t);
    ctx.heartbeat();
    if (doc.isEncrypted) {
      return { format: 'pdf', encrypted: true, pages: doc.getPageCount() };
    }
    return { format: 'pdf', encrypted: false, pages: doc.getPageCount() };
  } catch (e) {
    if (String(e).includes('encrypted') || String(e).includes('Encrypted')) {
      return { format: 'pdf', encrypted: true };
    }
    return { format: 'unknown' };
  }
}

export async function unlock(item: Item, password: string, ctx: Ctx): Promise<Item | 'wrong-password'> {
  const t = performance.now();
  const log: string[] = [];
  const consoleBak = { log: console.log, warn: console.warn, error: console.error };
  for (const k of ['log', 'warn', 'error'] as const) {
    console[k] = (...a: unknown[]) => {
      const msg = a.map(String).join(' ');
      log.push(msg);
      ctx.logs.push(msg);
    };
  }
  try {
    const qpdf = await getQpdf();
    ctx.heartbeat();
    (qpdf.FS as any).writeFile('/in.pdf', item.bytes);
    const code = qpdf.callMain([`--password=${password}`, '--decrypt', '/in.pdf', '/out.pdf']);
    ctx.timing('unlockMs', performance.now() - t);
    ctx.heartbeat();
    if (code !== 0 && code !== 3) {
      return 'wrong-password';
    }
    let out: Uint8Array | null = null;
    try {
      out = qpdf.FS.readFile('/out.pdf');
    } catch {
      return 'wrong-password';
    }
    if (!out || out.length === 0) return 'wrong-password';
    return { bytes: out, format: 'pdf' };
  } catch {
    return 'wrong-password';
  } finally {
    Object.assign(console, consoleBak);
  }
}

async function merge(items: Item[], ctx: Ctx): Promise<Item[]> {
  const t = performance.now();
  const { PDFDocument } = await import('pdf-lib');
  const outDoc = await PDFDocument.create();
  for (const item of items) {
    const doc = await PDFDocument.load(item.bytes);
    const pages = await outDoc.copyPages(doc, doc.getPageIndices());
    pages.forEach((p) => outDoc.addPage(p));
    ctx.heartbeat();
  }
  const bytes = await outDoc.save();
  ctx.timing('mergeMs', performance.now() - t);
  ctx.heartbeat();
  return [{ bytes: new Uint8Array(bytes), format: 'pdf' }];
}

async function split(item: Item, groupsJson: string, ctx: Ctx): Promise<Item[]> {
  const t = performance.now();
  const { PDFDocument } = await import('pdf-lib');
  const srcDoc = await PDFDocument.load(item.bytes);
  const groups: number[][] = JSON.parse(groupsJson);
  const results: Item[] = [];
  for (const group of groups) {
    const doc = await PDFDocument.create();
    const indices = group.map((p) => p - 1);
    const pages = await doc.copyPages(srcDoc, indices);
    pages.forEach((p) => doc.addPage(p));
    results.push({ bytes: new Uint8Array(await doc.save()), format: 'pdf' });
    ctx.heartbeat();
  }
  ctx.timing('splitMs', performance.now() - t);
  return results;
}

async function reorder(item: Item, orderJson: string, ctx: Ctx): Promise<Item[]> {
  const t = performance.now();
  const { PDFDocument } = await import('pdf-lib');
  const srcDoc = await PDFDocument.load(item.bytes);
  const order: number[] = JSON.parse(orderJson);
  const doc = await PDFDocument.create();
  const indices = order.map((p) => p - 1);
  const pages = await doc.copyPages(srcDoc, indices);
  pages.forEach((p) => doc.addPage(p));
  const bytes = await doc.save();
  ctx.timing('reorderMs', performance.now() - t);
  ctx.heartbeat();
  return [{ bytes: new Uint8Array(bytes), format: 'pdf' }];
}

const PAGE_SIZES: Record<string, [number, number]> = {
  a4: [595.28, 841.89],
  letter: [612, 792],
};

async function fromImages(items: Item[], pageSize: string, orientationsJson: string | undefined, ctx: Ctx): Promise<Item[]> {
  const t = performance.now();
  const { PDFDocument } = await import('pdf-lib');
  const doc = await PDFDocument.create();
  let orientations: number[] = [];
  if (orientationsJson) {
    try {
      orientations = JSON.parse(orientationsJson);
    } catch {
      orientations = [];
    }
  }

  for (const [idx, item] of items.entries()) {
    let embedded;
    let imgW: number;
    let imgH: number;

    if (item.format === 'jpeg') {
      const orientation = orientations[idx] ?? jpegOrientation(item.bytes);
      if (orientation !== 1) {
        // pdf-lib embeds raw JPEG bytes without applying EXIF orientation; bake upright pixels and strip EXIF.
        const pixels = await decode(item, ctx);
        const uprightJpg = await encode(pixels, 'jpeg', 92, ctx);
        embedded = await doc.embedJpg(uprightJpg);
        imgW = embedded.width;
        imgH = embedded.height;
      } else {
        try {
          embedded = await doc.embedJpg(item.bytes);
          imgW = embedded.width;
          imgH = embedded.height;
        } catch {
          const pixels = await decode(item, ctx);
          const pngBytes = await encode(pixels, 'png', 100, ctx);
          embedded = await doc.embedPng(pngBytes);
          imgW = embedded.width;
          imgH = embedded.height;
        }
      }
    } else if (item.format === 'png') {
      try {
        embedded = await doc.embedPng(item.bytes);
        imgW = embedded.width;
        imgH = embedded.height;
      } catch {
        const pixels = await decode(item, ctx);
        const pngBytes = await encode(pixels, 'png', 100, ctx);
        embedded = await doc.embedPng(pngBytes);
        imgW = embedded.width;
        imgH = embedded.height;
      }
    } else {
      const pixels = await decode(item, ctx);
      const pngBytes = await encode(pixels, 'png', 100, ctx);
      embedded = await doc.embedPng(pngBytes);
      imgW = embedded.width;
      imgH = embedded.height;
    }

    if (pageSize === 'fit' || !PAGE_SIZES[pageSize]) {
      const page = doc.addPage([imgW, imgH]);
      page.drawImage(embedded, { x: 0, y: 0, width: imgW, height: imgH });
    } else {
      const [pw, ph] = PAGE_SIZES[pageSize]!;
      const page = doc.addPage([pw, ph]);
      const scale = Math.min(pw / imgW, ph / imgH);
      const w = imgW * scale;
      const h = imgH * scale;
      const x = (pw - w) / 2;
      const y = (ph - h) / 2;
      page.drawImage(embedded, { x, y, width: w, height: h });
    }
    ctx.heartbeat();
  }

  const bytes = await doc.save();
  ctx.timing('fromImagesMs', performance.now() - t);
  return [{ bytes: new Uint8Array(bytes), format: 'pdf' }];
}

async function toImages(item: Item, to: Format, dpi: number, ctx: Ctx): Promise<Item[]> {
  const t = performance.now();
  const [pdfjsLib, { default: workerUrl }] = await Promise.all([
    import('pdfjs-dist'),
    import('pdfjs-dist/build/pdf.worker.min.mjs?url'),
  ]);
  pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl;

  const loadingTask = pdfjsLib.getDocument({
    data: item.bytes,
    useSystemFonts: true,
  });
  const doc = await loadingTask.promise;

  const n = doc.numPages;
  const scale = (dpi || 150) / 72;
  const results: Item[] = [];

  try {
    for (let i = 1; i <= n; i++) {
      const page = await doc.getPage(i);
      const viewport = page.getViewport({ scale });
      const w = Math.max(1, Math.round(viewport.width));
      const h = Math.max(1, Math.round(viewport.height));
      const canvas = new OffscreenCanvas(w, h);
      const canvasContext = canvas.getContext('2d')!;
      canvasContext.fillStyle = '#ffffff';
      canvasContext.fillRect(0, 0, w, h);
      await page.render({
        canvasContext: canvasContext as unknown as CanvasRenderingContext2D,
        viewport,
        canvas: canvas as unknown as HTMLCanvasElement,
      }).promise;
      const imgData = canvasContext.getImageData(0, 0, w, h);
      const bytes = await encode(new ImageData(imgData.data, w, h), to, 90, ctx);
      results.push({ bytes, format: to });
      ctx.heartbeat();
    }
  } finally {
    await doc.cleanup();
    await loadingTask.destroy();
  }

  ctx.timing('toImagesMs', performance.now() - t);
  return results;
}

async function compress(item: Item, ctx: Ctx): Promise<Item[]> {
  const t = performance.now();
  const log: string[] = [];
  const consoleBak = { log: console.log, warn: console.warn, error: console.error };
  for (const k of ['log', 'warn', 'error'] as const) {
    console[k] = (...a: unknown[]) => {
      const msg = a.map(String).join(' ');
      log.push(msg);
      ctx.logs.push(msg);
    };
  }
  try {
    const qpdf = await getQpdf();
    ctx.heartbeat();
    (qpdf.FS as any).writeFile('/in.pdf', item.bytes);
    const code = qpdf.callMain(['--recompress-flate', '--compression-level=9', '--object-streams=generate', '/in.pdf', '/out.pdf']);
    ctx.timing('compressMs', performance.now() - t);
    ctx.heartbeat();
    if (code !== 0 && code !== 3) {
      throw damaged(log.join('\n') || `qpdf exited with code ${code}`);
    }
    const out = qpdf.FS.readFile('/out.pdf');
    // If not smaller, keep the original bytes
    const finalBytes = out.length < item.bytes.length ? out : item.bytes;
    return [{ bytes: finalBytes, format: 'pdf' }];
  } catch (e) {
    if (e instanceof RefractError) throw e;
    throw damaged(e instanceof Error ? e.message : String(e));
  } finally {
    Object.assign(console, consoleBak);
  }
}

export async function run(op: string, items: Item[], opts: Options, ctx: Ctx): Promise<Item[]> {
  const [first] = items;
  if (!first) throw new RefractError('Internal', 'no input', '');

  if (op === 'pdf.merge') return merge(items, ctx);
  if (op === 'pdf.split') return split(first, String(opts.groups ?? '[]'), ctx);
  if (op === 'pdf.reorder') return reorder(first, String(opts.order ?? '[]'), ctx);
  if (op === 'pdf.fromImages') return fromImages(items, String(opts.pageSize ?? 'fit'), opts.orientations ? String(opts.orientations) : undefined, ctx);
  if (op === 'pdf.toImages') return toImages(first, opts.to as Format, Number(opts.dpi ?? 150), ctx);
  if (op === 'pdf.compress') return compress(first, ctx);

  throw new RefractError('Internal', `unknown PDF operation ${op}`, '');
}
