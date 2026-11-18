/**
 * In-memory client-side sample file generators.
 * Allows users to immediately test-drive Refract's WebAssembly engine without needing to search their disk.
 */

/** Generate a synthetic high-resolution sample PNG image with geometric prism refraction graphics. */
export function createSampleImage(suffix = ''): Promise<File> {
  return new Promise((resolve) => {
    const canvas = document.createElement('canvas');
    canvas.width = 1200;
    canvas.height = 800;
    const ctx = canvas.getContext('2d');
    if (!ctx) {
      resolve(new File(['sample'], `sample${suffix}.png`, { type: 'image/png' }));
      return;
    }

    // Rich darkroom/prism gradient backdrop
    const grad = ctx.createLinearGradient(0, 0, 1200, 800);
    grad.addColorStop(0, '#0f172a');
    grad.addColorStop(0.5, '#1e293b');
    grad.addColorStop(1, '#090d16');
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, 1200, 800);

    // Subtle technical grid
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.05)';
    ctx.lineWidth = 1;
    for (let x = 0; x < 1200; x += 40) {
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, 800);
      ctx.stroke();
    }
    for (let y = 0; y < 800; y += 40) {
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(1200, y);
      ctx.stroke();
    }

    // Refract Prism Light Beam
    const beam = ctx.createLinearGradient(300, 200, 900, 600);
    beam.addColorStop(0, 'rgba(11, 87, 208, 0.7)');
    beam.addColorStop(0.3, 'rgba(56, 189, 248, 0.8)');
    beam.addColorStop(0.7, 'rgba(236, 72, 153, 0.7)');
    beam.addColorStop(1, 'rgba(234, 179, 8, 0.8)');
    ctx.fillStyle = beam;
    ctx.beginPath();
    ctx.moveTo(600, 180);
    ctx.lineTo(1050, 620);
    ctx.lineTo(150, 620);
    ctx.closePath();
    ctx.fill();

    // Central prism triangle outline
    ctx.strokeStyle = '#ffffff';
    ctx.lineWidth = 4;
    ctx.beginPath();
    ctx.moveTo(600, 240);
    ctx.lineTo(820, 560);
    ctx.lineTo(380, 560);
    ctx.closePath();
    ctx.stroke();

    // Technical Typography
    ctx.fillStyle = '#ffffff';
    ctx.font = 'bold 36px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText('REFRACT LOCAL ENGINE', 600, 420);

    ctx.font = '16px ui-monospace, Menlo, monospace';
    ctx.fillStyle = '#94a3b8';
    ctx.fillText('1200 × 800 · 24-BIT RGB · WASM TEST SAMPLE', 600, 460);

    canvas.toBlob((blob) => {
      if (blob) {
        resolve(new File([blob], `sample-refract-photo${suffix}.png`, { type: 'image/png' }));
      } else {
        resolve(new File(['sample'], `sample${suffix}.png`, { type: 'image/png' }));
      }
    }, 'image/png');
  });
}

/** Generate a multi-page sample PDF using pdf-lib entirely in browser memory. */
export async function createSamplePdf(suffix = ''): Promise<File> {
  const { PDFDocument, rgb, StandardFonts } = await import('pdf-lib');
  const doc = await PDFDocument.create();
  const font = await doc.embedFont(StandardFonts.HelveticaBold);
  const regular = await doc.embedFont(StandardFonts.Helvetica);

  // Page 1
  const page1 = doc.addPage([600, 400]);
  page1.drawText('Refract Sample Document', {
    x: 50,
    y: 330,
    size: 24,
    font,
    color: rgb(0.04, 0.34, 0.82),
  });
  page1.drawText('Page 1 of 2 — Processed 100% locally via WebAssembly.', {
    x: 50,
    y: 290,
    size: 13,
    font: regular,
    color: rgb(0.15, 0.15, 0.15),
  });
  page1.drawText('This sample document can be used to test PDF Merge, Split, Reorder, or Compress.', {
    x: 50,
    y: 260,
    size: 11,
    font: regular,
    color: rgb(0.35, 0.35, 0.35),
  });
  page1.drawText('No files leave your browser. Zero cloud telemetry.', {
    x: 50,
    y: 235,
    size: 11,
    font: regular,
    color: rgb(0.35, 0.35, 0.35),
  });

  // Page 2
  const page2 = doc.addPage([600, 400]);
  page2.drawText('Page 2 — Refract Core Architecture', {
    x: 50,
    y: 330,
    size: 20,
    font,
    color: rgb(0.1, 0.1, 0.1),
  });
  page2.drawText('Local-first document transformation without server upload queues or file size fees.', {
    x: 50,
    y: 290,
    size: 13,
    font: regular,
    color: rgb(0.2, 0.2, 0.2),
  });
  page2.drawText('Supported PDF operations: QPDF lossless compression, page extraction, image rendering.', {
    x: 50,
    y: 260,
    size: 11,
    font: regular,
    color: rgb(0.35, 0.35, 0.35),
  });

  const bytes = await doc.save();
  return new File([bytes as unknown as BlobPart], `sample-refract-document${suffix}.pdf`, { type: 'application/pdf' });
}

/** Generate a pleasant 2.5-second harmonic audio chime in WAV format using raw PCM synthesis. */
export function createSampleAudio(suffix = ''): File {
  const sampleRate = 44100;
  const duration = 2.5; // 2.5 seconds
  const numSamples = Math.floor(sampleRate * duration);
  const buffer = new ArrayBuffer(44 + numSamples * 2);
  const view = new DataView(buffer);

  // RIFF chunk descriptor
  view.setUint32(0, 0x52494646, false); // "RIFF"
  view.setUint32(4, 36 + numSamples * 2, true);
  view.setUint32(8, 0x57415645, false); // "WAVE"

  // fmt subchunk
  view.setUint32(12, 0x666d7420, false); // "fmt "
  view.setUint32(16, 16, true); // Subchunk1Size (16 for PCM)
  view.setUint16(20, 1, true); // AudioFormat (1 = PCM)
  view.setUint16(22, 1, true); // NumChannels (1 = mono)
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true); // ByteRate (SampleRate * NumChannels * BitsPerSample/8)
  view.setUint16(32, 2, true); // BlockAlign (NumChannels * BitsPerSample/8)
  view.setUint16(34, 16, true); // BitsPerSample (16 bits)

  // data subchunk
  view.setUint32(36, 0x64617461, false); // "data"
  view.setUint32(40, numSamples * 2, true);

  // Synthesize chord: C major pentatonic chime (C5: 523Hz, E5: 659Hz, G5: 784Hz) with exponential decay
  for (let i = 0; i < numSamples; i++) {
    const t = i / sampleRate;
    const env = Math.exp(-2.2 * t);
    const val =
      0.45 * Math.sin(2 * Math.PI * 523.25 * t) +
      0.35 * Math.sin(2 * Math.PI * 659.25 * t) +
      0.25 * Math.sin(2 * Math.PI * 783.99 * t);
    const sample = Math.max(-1, Math.min(1, val * env)) * 32767;
    view.setInt16(44 + i * 2, sample, true);
  }

  return new File([buffer], `sample-refract-tone${suffix}.wav`, { type: 'audio/wav' });
}
