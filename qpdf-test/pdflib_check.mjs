// Does pdf-lib need qpdf? Compare a locked input with qpdf's unlocked output.
import { readFileSync } from 'node:fs';
import { PDFDocument } from 'pdf-lib';

async function tryMerge(label, path) {
  try {
    const src = await PDFDocument.load(readFileSync(path));
    const out = await PDFDocument.create();
    (await out.copyPages(src, src.getPageIndices())).forEach((p) => out.addPage(p));
    const merged = await PDFDocument.load(await out.save());
    console.log(`${label}: OK, merged ${merged.getPageCount()} pages`);
  } catch (e) {
    console.log(`${label}: ERROR ${e.constructor.name}: ${String(e.message).split('\n')[0]}`);
  }
}

await tryMerge('locked aes256 (no qpdf)', 'fixtures/aes256.pdf');
await tryMerge('unlocked aes256 (after qpdf)', 'out/aes256.pdf');
await tryMerge('unlocked rc4-128 (after qpdf)', 'out/rc4_128.pdf');
