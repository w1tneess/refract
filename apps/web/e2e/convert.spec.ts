import { expect, test } from '@playwright/test';
import { addFiles, card, download, dropFile, expectJpeg, hasFixture, isBlue, isRed, pixel, waitForState } from './helpers.ts';

test.beforeEach(async ({ page }) => {
  await page.goto('/');
});

test('drop → convert → download (plain HEIC, real drag and drop)', async ({ page }) => {
  await dropFile(page, 'plain.heic');
  const c = card(page);
  await waitForState(c, 'Saved');
  const out = await download(page, c);
  expect(out.name).toBe('plain (converted).jpg');
  expectJpeg(out.bytes, 400, 300, [500, 60_000]);
  expect(isRed(await pixel(page, out.bytes, 10, 10))).toBe(true);
  expect(isBlue(await pixel(page, out.bytes, 390, 290))).toBe(true);
});

test('iPhone HEIC converts to a JPEG of the same displayed size', async ({ page }) => {
  test.skip(!hasFixture('iphone.heic'), 'fixtures/iphone.heic missing: add a real iPhone photo (see fixtures/README.md)');
  await addFiles(page, ['iphone.heic']);
  const c = card(page);
  await waitForState(c, 'Saved', 60_000);
  const out = await download(page, c);
  const { width, height } = JSON.parse((await import('node:fs')).readFileSync(new URL('../../../fixtures/iphone.json', import.meta.url), 'utf8'));
  expectJpeg(out.bytes, width, height, [100_000, 15_000_000]);
});

test('rotated HEIC is rotated once (irot), then has no orientation left', async ({ page }) => {
  await addFiles(page, ['rotated.heic']);
  const c = card(page);
  await waitForState(c, 'Saved');
  const out = await download(page, c);
  expectJpeg(out.bytes, 300, 400, [500, 60_000]);
  // Stored top-left red block, rotated 90° clockwise, ends up top-right.
  expect(isRed(await pixel(page, out.bytes, 290, 10))).toBe(true);
  expect(isBlue(await pixel(page, out.bytes, 10, 10))).toBe(true);
});

test('a .jpg that is really a HEIC is detected by its bytes and converted, with a notice', async ({ page }) => {
  await addFiles(page, ['really-heic.jpg']);
  const c = card(page);
  await waitForState(c, 'Saved');
  await expect(c.locator('.job-notice')).toContainText('named .jpg, but it is really a HEIC image');
  const out = await download(page, c);
  expect(out.name).toBe('really-heic (converted).jpg');
  expectJpeg(out.bytes, 400, 300, [500, 60_000]);
});

test('Bengali/emoji file name is kept in the output name', async ({ page }) => {
  await addFiles(page, ['ছবি 😀.heic']);
  const c = card(page);
  await waitForState(c, 'Saved');
  const out = await download(page, c);
  expect(out.name).toBe('ছবি 😀 (converted).jpg');
  expectJpeg(out.bytes, 400, 300, [500, 60_000]);
});

test('two files with the same name in one batch get unique output names', async ({ page }) => {
  await addFiles(page, ['plain.heic']);
  await addFiles(page, ['plain.heic']);
  await waitForState(card(page, 0), 'Saved');
  await waitForState(card(page, 1), 'Saved');
  expect((await download(page, card(page, 0))).name).toBe('plain (converted).jpg');
  expect((await download(page, card(page, 1))).name).toBe('plain (converted) (2).jpg');
});

test('0-byte file is rejected with a plain message', async ({ page }) => {
  await addFiles(page, ['empty.heic']);
  const c = card(page);
  await waitForState(c, 'Rejected');
  await expect(c.locator('.job-error')).toContainText("Couldn't convert empty.heic: the file is empty (0 bytes). Pick the original file again.");
  await expect(c.getByRole('link', { name: /^Download/ })).toHaveCount(0);
});

test('truncated HEIC fails as damaged, with the raw error behind Details', async ({ page }) => {
  await addFiles(page, ['truncated.heic']);
  const c = card(page);
  await waitForState(c, 'Failed');
  await expect(c.locator('.job-error > p')).toContainText('the file is damaged');
  const details = c.locator('details');
  await expect(details.locator('pre')).not.toBeVisible();
  await details.locator('summary').click();
  await expect(details.locator('pre')).toContainText('Corrupt');
});

test('cancel mid-job ends Canceled (not Failed) with no download', async ({ page }) => {
  await addFiles(page, ['large.heic']);
  const c = card(page);
  await waitForState(c, 'Running');
  await c.getByRole('button', { name: /^Cancel/ }).click();
  await waitForState(c, 'Canceled');
  await expect(c.locator('.job-error')).toHaveCount(0);
  await expect(c.getByRole('link', { name: /^Download/ })).toHaveCount(0);
  await expect(page.locator('[data-batch]')).toHaveAttribute('data-batch', 'finished');
});
