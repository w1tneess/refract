import { expect, test } from '@playwright/test';
import { addFiles, card, download, expectAudio, waitForState } from './helpers.ts';

test.describe('M3 Audio: convert, trim, lazy ffmpeg load', () => {
  test('convert-audio: WAV to MP3', async ({ page }) => {
    await page.goto('/#convert-audio');
    await addFiles(page, ['sound.wav']);
    const c = card(page);
    await waitForState(c, 'Saved', 60_000);
    const out = await download(page, c);
    expect(out.name).toBe('sound (converted).mp3');
    expectAudio(out.bytes, 'mp3');
  });

  test('convert-audio: MP3 to WAV', async ({ page }) => {
    await page.goto('/#convert-audio');
    await page.locator('fieldset.options select').first().selectOption('wav');
    await addFiles(page, ['sound.mp3']);
    const c = card(page);
    await waitForState(c, 'Saved', 60_000);
    const out = await download(page, c);
    expect(out.name).toBe('sound (converted).wav');
    expectAudio(out.bytes, 'wav');
  });

  test('convert-audio: M4A to OGG', async ({ page }) => {
    await page.goto('/#convert-audio');
    await page.locator('fieldset.options select').first().selectOption('ogg');
    await addFiles(page, ['sound.m4a']);
    const c = card(page);
    await waitForState(c, 'Saved', 60_000);
    const out = await download(page, c);
    expect(out.name).toBe('sound (converted).ogg');
    expectAudio(out.bytes, 'ogg');
  });

  test('convert-audio: OGG to MP3', async ({ page }) => {
    await page.goto('/#convert-audio');
    await page.locator('fieldset.options select').first().selectOption('mp3');
    await addFiles(page, ['sound.ogg']);
    const c = card(page);
    await waitForState(c, 'Saved', 60_000);
    const out = await download(page, c);
    expect(out.name).toBe('sound (converted).mp3');
    expectAudio(out.bytes, 'mp3');
  });

  test('trim-audio: keeps the segment between start and end times', async ({ page }) => {
    await page.goto('/#trim-audio');
    const startInput = page.locator('fieldset.options input[type=number]').nth(0);
    const endInput = page.locator('fieldset.options input[type=number]').nth(1);
    await startInput.fill('1');
    await endInput.fill('2.5');
    await addFiles(page, ['sound.wav']);
    const c = card(page);
    await waitForState(c, 'Saved', 60_000);
    const out = await download(page, c);
    expect(out.name).toBe('sound_trimmed.wav');
    expectAudio(out.bytes, 'wav');
  });

  test('lazy ffmpeg load is measured in job timings', async ({ page }) => {
    await page.goto('/#convert-audio');
    await addFiles(page, ['sound.wav']);
    const c = card(page);
    await waitForState(c, 'Saved', 60_000);
    const timingsAttr = await c.getAttribute('data-timings');
    expect(timingsAttr).toBeTruthy();
    const timings = JSON.parse(timingsAttr!);
    expect(timings).toHaveProperty('wasmLoadMs');
    expect(timings.wasmLoadMs).toBeGreaterThan(0);
  });
});
