import { mkdirSync, writeFileSync } from 'node:fs';
import { test } from '@playwright/test';
import { addFiles, card, hasFixture, waitForState } from './helpers.ts';

// Measures worker load and conversion time per fixture, for docs/phase-1-report.md.
// Each job spawns a fresh worker, so every run includes worker start + WASM instantiation.
const FIXTURES = ['plain.heic', 'rotated.heic', 'large.heic', 'iphone.heic'];
const RUNS = 3;

test('measure timings per fixture', async ({ page, browserName }) => {
  test.setTimeout(240_000);
  await page.goto('/');
  const rows: Record<string, Record<string, number>[]> = {};
  let i = 0;
  for (const name of FIXTURES.filter(hasFixture)) {
    rows[name] = [];
    for (let r = 0; r < RUNS; r++) {
      await addFiles(page, [name]);
      const c = card(page, i++);
      await waitForState(c, 'Saved', 120_000);
      rows[name].push(JSON.parse((await c.getAttribute('data-timings'))!));
    }
  }
  mkdirSync('test-results', { recursive: true });
  writeFileSync(`test-results/timings-${browserName}.json`, JSON.stringify(rows, null, 2));
  console.log(browserName, JSON.stringify(rows));
});
