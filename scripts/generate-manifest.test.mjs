import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import { validateManifest, rsFloat, generateRust } from './generate-manifest.mjs';
const manifest = JSON.parse(fs.readFileSync(new URL('../specs/tool-manifest.json', import.meta.url)));
const schema = JSON.parse(fs.readFileSync(new URL('../specs/tool-manifest.schema.json', import.meta.url)));
test('schema enforces Desktop budget', () => {
  const copy = structuredClone(manifest);
  copy.memoryBudgetMB.desktop = 128;
  assert.throws(() => validateManifest(copy, schema), /256/);
  copy.memoryBudgetMB.desktop = 8192;
  assert.doesNotThrow(() => validateManifest(copy, schema));
});
test('default must be available on each option platform', () => {
  const copy = structuredClone(manifest);
  const option = copy.tools[0].options.find(o => o.type === 'select');
  option.choices.find(c => c.value === option.default).platforms = ['web'];
  assert.throws(() => validateManifest(copy, schema), /default unavailable on desktop/);
});
test('Rust float literals compile including exponent form', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'refract-floats-'));
  try {
    const file = path.join(dir, 'main.rs');
    fs.writeFileSync(file, `fn main() { let _: [f64; 3] = [${[1, 0.1, 1e-7].map(rsFloat).join(',')}]; }`);
    const result = spawnSync('rustc', [file, '-o', path.join(dir, 'test')], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.error?.message ?? result.stderr);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('Rust choices preserve platform restrictions in Desktop lookup slices', () => {
  const copy = structuredClone(manifest);
  const choice = copy.tools[0].options.find(o => o.key === 'to').choices.find(c => c.value === 'webp');
  choice.platforms = ['web'];
  const rust = generateRust(copy);
  const desktop = rust.match(/desktop_choices: &\[([\s\S]*?)\n                \],/)[1];
  const web = rust.match(/web_choices: &\[([\s\S]*?)\n                \],/)[1];
  assert.doesNotMatch(desktop, /value: "webp"/);
  assert.match(web, /value: "webp"/);
  assert.match(desktop, /value: "jpeg"/);
});
