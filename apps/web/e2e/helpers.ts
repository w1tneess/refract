import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { expect, type Locator, type Page } from '@playwright/test';
import { readJpegInfo, detect, probeImage, type Format } from '@refract/core';

export const fixture = (name: string) => fileURLToPath(new URL(`../../../fixtures/${name}`, import.meta.url));
export const hasFixture = (name: string) => existsSync(fixture(name));

export const card = (page: Page, i = 0) => page.locator('li.job').nth(i);

export async function addFiles(page: Page, names: string[]) {
  await page.locator('input[type=file]').setInputFiles(names.map(fixture));
}

/** A real drag-and-drop: build a DataTransfer in the page and drop it on the drop zone. */
export async function dropFile(page: Page, name: string) {
  const bytes = [...readFileSync(fixture(name))];
  const fileName = name;
  const dt = await page.evaluateHandle(({ bytes, fileName }) => {
    const dt = new DataTransfer();
    dt.items.add(new File([new Uint8Array(bytes)], fileName));
    return dt;
  }, { bytes, fileName });
  await page.getByTestId('dropzone').dispatchEvent('drop', { dataTransfer: dt });
}

export async function waitForState(c: Locator, state: string, timeout = 30_000) {
  await expect(c).toHaveAttribute('data-state', state, { timeout });
}

/** Click Download and return the file name and bytes the browser saved. */
export async function download(page: Page, c: Locator) {
  const [dl] = await Promise.all([page.waitForEvent('download'), c.getByRole('link', { name: /^Download/ }).click()]);
  const bytes = new Uint8Array(readFileSync((await dl.path())!));
  return { name: dl.suggestedFilename(), bytes };
}

/** Type, dimensions and size range: not exact bytes, because encoders change across versions. */
export function expectJpeg(bytes: Uint8Array, width: number, height: number, sizeRange: [number, number]) {
  expect(detect(bytes)).toBe('jpeg');
  const info = readJpegInfo(bytes);
  expect([info.width, info.height]).toEqual([width, height]);
  expect(info.hasExif).toBe(false);
  expect(info.complete).toBe(true);
  expect(bytes.length).toBeGreaterThanOrEqual(sizeRange[0]);
  expect(bytes.length).toBeLessThanOrEqual(sizeRange[1]);
}

export function expectImage(bytes: Uint8Array, format: Format, width: number, height: number, sizeRange?: [number, number]) {
  expect(detect(bytes)).toBe(format);
  const info = probeImage(format, bytes);
  expect([info.width, info.height]).toEqual([width, height]);
  if (format === 'jpeg') {
    expect(readJpegInfo(bytes).hasExif).toBe(false);
  }
  if (sizeRange) {
    expect(bytes.length).toBeGreaterThanOrEqual(sizeRange[0]);
    expect(bytes.length).toBeLessThanOrEqual(sizeRange[1]);
  }
}

/** Decode an image in the page and read the colour at (x, y). */
export async function pixel(page: Page, bytes: Uint8Array, x: number, y: number, mime?: string) {
  const detected = detect(bytes);
  const m = mime ?? (detected ? `image/${detected}` : 'image/jpeg');
  return page.evaluate(async ({ bytes, x, y, m }) => {
    const bmp = await createImageBitmap(new Blob([new Uint8Array(bytes)], { type: m }));
    const c = new OffscreenCanvas(bmp.width, bmp.height);
    const ctx = c.getContext('2d')!;
    ctx.drawImage(bmp, 0, 0);
    const [r, g, b] = ctx.getImageData(x, y, 1, 1).data;
    return { r: r!, g: g!, b: b! };
  }, { bytes: [...bytes], x, y, m });
}

export const isRed = (p: { r: number; g: number; b: number }) => p.r > 180 && p.b < 80;
export const isBlue = (p: { r: number; g: number; b: number }) => p.b > 180 && p.r < 80;
