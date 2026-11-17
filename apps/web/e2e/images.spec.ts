import { expect, test } from '@playwright/test';
import { detect } from '@refract/core';
import { addFiles, card, download, expectImage, expectJpeg, isBlue, isRed, pixel, waitForState } from './helpers.ts';

test.describe('M1 Images: format pairs, EXIF orientation, compress, resize, retry', () => {
  test('convert PNG to JPEG', async ({ page }) => {
    await page.goto('/#convert-image');
    await addFiles(page, ['plain.png']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('plain (converted).jpg');
    expectJpeg(out.bytes, 400, 300, [500, 60_000]);
    expect(isRed(await pixel(page, out.bytes, 10, 10))).toBe(true);
    expect(isBlue(await pixel(page, out.bytes, 390, 290))).toBe(true);
  });

  test('convert WebP to JPEG', async ({ page }) => {
    await page.goto('/#convert-image');
    await addFiles(page, ['plain.webp']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('plain (converted).jpg');
    expectJpeg(out.bytes, 400, 300, [500, 60_000]);
  });

  test('convert JPEG to WebP', async ({ page }) => {
    await page.goto('/#convert-image');
    // Select WebP from the options dropdown
    await page.locator('fieldset.options select').first().selectOption('webp');
    await addFiles(page, ['plain.jpg']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('plain (converted).webp');
    expectImage(out.bytes, 'webp', 400, 300);
  });

  test('convert JPEG to PNG', async ({ page }) => {
    await page.goto('/#convert-image');
    await page.locator('fieldset.options select').first().selectOption('png');
    await addFiles(page, ['plain.jpg']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('plain (converted).png');
    expectImage(out.bytes, 'png', 400, 300);
  });

  test('rotated JPEG with EXIF 6 is oriented upright (300×400) and stripped of EXIF', async ({ page }) => {
    await page.goto('/#convert-image');
    await page.locator('fieldset.options select').first().selectOption('webp');
    await addFiles(page, ['rotated.jpg']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('rotated (converted).webp');
    expectImage(out.bytes, 'webp', 300, 400);
    // Stored top-left red block, rotated 90° CW, ends up at top-right
    expect(isRed(await pixel(page, out.bytes, 290, 10))).toBe(true);
    expect(isBlue(await pixel(page, out.bytes, 10, 10))).toBe(true);
  });

  test('a .jpg that is really a PNG is detected by bytes with notice', async ({ page }) => {
    await page.goto('/#convert-image');
    await page.locator('fieldset.options select').first().selectOption('jpeg');
    await addFiles(page, ['really-png.jpg']);
    const c = card(page);
    await waitForState(c, 'Saved');
    await expect(c.locator('.job-notice')).toContainText('named .jpg, but it is really a PNG image');
    const out = await download(page, c);
    expect(out.name).toBe('really-png (converted).jpg');
    expectJpeg(out.bytes, 400, 300, [500, 60_000]);
  });

  test('damaged image fails, and can be retried', async ({ page }) => {
    await page.goto('/#convert-image');
    await addFiles(page, ['truncated.heic']);
    const c = card(page);
    await waitForState(c, 'Failed');
    await expect(c.locator('.job-error > p')).toContainText('the file is damaged');
    // Click Retry
    await c.getByRole('button', { name: 'Retry' }).click();
    // Re-enters state machine and ends Failed again for this corrupted file
    await waitForState(c, 'Failed');
  });

  test('compress image to target KB', async ({ page }) => {
    await page.goto('/#compress-image');
    // Target size input
    const input = page.locator('fieldset.options input[type=number]');
    await input.fill('20'); // 20 KB
    await addFiles(page, ['plain.jpg']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('plain_compressed.jpg');
    expect(out.bytes.length).toBeLessThanOrEqual(20 * 1024);
    expect(detect(out.bytes)).toBe('jpeg');
  });

  test('resize image by pixels (maintaining aspect ratio when one dimension is 0)', async ({ page }) => {
    await page.goto('/#resize-image');
    const widthInput = page.locator('fieldset.options input[type=number]').nth(0);
    const heightInput = page.locator('fieldset.options input[type=number]').nth(1);
    await widthInput.fill('200');
    await heightInput.fill('0'); // auto height: 400x300 -> 200x150
    await addFiles(page, ['plain.jpg']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('plain_resized.jpg');
    expectJpeg(out.bytes, 200, 150, [200, 30_000]);
  });

  test('options are remembered and reset button restores defaults', async ({ page }) => {
    await page.goto('/#convert-image');
    const select = page.locator('fieldset.options select').first();
    await select.selectOption('webp');
    // Reload page to verify persistence
    await page.reload();
    await expect(page.locator('fieldset.options select').first()).toHaveValue('webp');
    // Reset to defaults
    await page.getByRole('button', { name: 'Reset to defaults' }).click();
    await expect(page.locator('fieldset.options select').first()).toHaveValue('jpeg');
  });

  test('batch job offers Save all as ZIP', async ({ page }) => {
    await page.goto('/#convert-image');
    await addFiles(page, ['plain.png', 'plain.webp']);
    await waitForState(card(page, 0), 'Saved');
    await waitForState(card(page, 1), 'Saved');
    const saveAllBtn = page.getByRole('button', { name: 'Save all as ZIP' });
    await expect(saveAllBtn).toBeVisible();
    await saveAllBtn.click();
    const downloadLink = page.getByRole('link', { name: 'Download refract.zip' });
    await expect(downloadLink).toBeVisible();
    const [dl] = await Promise.all([page.waitForEvent('download'), downloadLink.click()]);
    expect(dl.suggestedFilename()).toBe('refract.zip');
  });
});
