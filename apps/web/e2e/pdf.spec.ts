import { expect, test } from '@playwright/test';
import { addFiles, card, download, expectPdf, waitForState } from './helpers.ts';

test.describe('M2 PDF: merge, split, reorder, images-to-pdf, pdf-to-images, compress, password', () => {
  test('merge-pdf: combines multiple PDFs into one with all pages', async ({ page }) => {
    await page.goto('/#merge-pdf');
    await addFiles(page, ['plain.pdf', '5pages.pdf']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('plain_merged.pdf');
    await expectPdf(out.bytes, 8); // 3 + 5 pages
  });

  test('split-pdf: splits a PDF by page ranges into a ZIP of PDFs', async ({ page }) => {
    await page.goto('/#split-pdf');
    const rangesInput = page.locator('fieldset.options input[type=text]');
    await rangesInput.fill('1-2, 4-5');
    await addFiles(page, ['5pages.pdf']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('5pages_split.zip');
    const text = new TextDecoder('latin1').decode(out.bytes);
    expect(text).toContain('5pages-p1-2.pdf');
    expect(text).toContain('5pages-p4-5.pdf');
  });

  test('reorder-pdf: reorders pages of a PDF', async ({ page }) => {
    await page.goto('/#reorder-pdf');
    const orderInput = page.locator('fieldset.options input[type=text]');
    await orderInput.fill('3, 1, 2');
    await addFiles(page, ['plain.pdf']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('plain_reordered.pdf');
    await expectPdf(out.bytes, 3);
  });

  test('images-to-pdf: creates a PDF from image files', async ({ page }) => {
    await page.goto('/#images-to-pdf');
    await addFiles(page, ['plain.png', 'plain.jpg']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('plain.pdf');
    await expectPdf(out.bytes, 2);
  });

  test('images-to-pdf: applies EXIF orientation for rotated JPEG', async ({ page }) => {
    await page.goto('/#images-to-pdf');
    await page.locator('fieldset.options select').first().selectOption('fit');
    await addFiles(page, ['rotated.jpg']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    await expectPdf(out.bytes, 1);
    const { PDFDocument } = await import('pdf-lib');
    const doc = await PDFDocument.load(out.bytes);
    const { width, height } = doc.getPage(0).getSize();
    expect([Math.round(width), Math.round(height)]).toEqual([300, 400]);
  });

  test('pdf-to-images: renders PDF pages to images inside a ZIP', async ({ page }) => {
    await page.goto('/#pdf-to-images');
    await addFiles(page, ['plain.pdf']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('plain_pages.zip');
    const text = new TextDecoder('latin1').decode(out.bytes);
    expect(text).toContain('plain-p1.png');
    expect(text).toContain('plain-p2.png');
    expect(text).toContain('plain-p3.png');
  });

  test('compress-pdf: lossless compression with qpdf', async ({ page }) => {
    await page.goto('/#compress-pdf');
    await addFiles(page, ['plain.pdf']);
    const c = card(page);
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('plain_compressed.pdf');
    await expectPdf(out.bytes, 3);
    // Losslessly recompressed flate + object streams should shrink the 1122 byte sample
    expect(out.bytes.length).toBeLessThan(1122);
  });

  test('owner-only PDF opens without password prompt', async ({ page }) => {
    await page.goto('/#compress-pdf');
    await addFiles(page, ['owner-only.pdf']);
    const c = card(page);
    // Should pass directly to Saved without staying in NeedsPassword
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('owner-only_compressed.pdf');
  });

  test('locked PDF asks for password, rejects wrong password, unlocks with right one', async ({ page }) => {
    await page.goto('/#compress-pdf');
    await addFiles(page, ['locked.pdf']);
    const c = card(page);
    await waitForState(c, 'NeedsPassword');
    const pwForm = c.locator('form.password');
    await expect(pwForm).toBeVisible();

    // Enter wrong password
    const pwInput = pwForm.locator('input[type=password]');
    await pwInput.fill('wrong-password-xyz');
    await pwForm.getByRole('button', { name: 'Unlock' }).click();

    // Still in NeedsPassword with invalid state / message
    await waitForState(c, 'NeedsPassword');
    await expect(pwForm.locator('label').first()).toContainText('password is wrong');

    // Enter right password
    await pwInput.fill('user-pw-123');
    await pwForm.getByRole('button', { name: 'Unlock' }).click();

    // Now completes
    await waitForState(c, 'Saved');
    const out = await download(page, c);
    expect(out.name).toBe('locked_compressed.pdf');
    await expectPdf(out.bytes, 3);
  });
});
