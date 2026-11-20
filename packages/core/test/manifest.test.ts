import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  FORMATS,
  LIMITS,
  MANIFEST_FORMATS,
  MANIFEST_VERSION,
  manifestOptionsForPlatform,
  resolveOptions,
  TOOL_IDS,
  TOOL_MANIFEST,
  TOOL_MANIFEST_LIST,
  TOOLS,
  type Format,
  type ToolId,
} from '../src/index.ts';

const ROOT = path.resolve(import.meta.dirname, '..', '..', '..');
const RAW_MANIFEST = JSON.parse(fs.readFileSync(path.join(ROOT, 'specs', 'tool-manifest.json'), 'utf8'));
const RAW_SCHEMA = JSON.parse(fs.readFileSync(path.join(ROOT, 'specs', 'tool-manifest.schema.json'), 'utf8'));
const RUST_GENERATED = fs.readFileSync(path.join(ROOT, 'crates', 'refract-core', 'src', 'generated_manifest.rs'), 'utf8');

const EXPECTED_TOOL_IDS: ToolId[] = [
  'convert-image',
  'compress-image',
  'resize-image',
  'merge-pdf',
  'split-pdf',
  'reorder-pdf',
  'images-to-pdf',
  'pdf-to-images',
  'compress-pdf',
  'convert-audio',
  'trim-audio',
];

describe('Canonical Tool Manifest & Cross-Language Parity', () => {
  it('preserves all 11 canonical tool IDs uniquely and in order', () => {
    expect(TOOL_IDS).toEqual(EXPECTED_TOOL_IDS);
    expect(new Set(TOOL_IDS).size).toBe(11);
    expect(TOOL_MANIFEST_LIST.map((t) => t.id)).toEqual(EXPECTED_TOOL_IDS);
    expect(Object.keys(TOOLS)).toEqual(EXPECTED_TOOL_IDS);
    expect(RAW_SCHEMA.$schema).toContain('json-schema.org');
  });

  it('validates every tool formats, option defaults, constraints, showIf refs, and limits', () => {
    const validFormats = new Set(Object.keys(FORMATS));
    const validLimits = new Set(Object.keys(LIMITS));

    for (const tool of TOOL_MANIFEST_LIST) {
      expect(validLimits.has(tool.limitsKey)).toBe(true);
      expect(tool.inputs.min).toBeGreaterThanOrEqual(1);
      if (tool.inputs.max !== null) {
        expect(tool.inputs.max).toBeGreaterThanOrEqual(tool.inputs.min);
      }

      // Platform format capabilities match format matrix
      for (const platform of ['web', 'desktop'] as const) {
        expect(tool.acceptedFormats[platform].length).toBeGreaterThan(0);
        for (const f of tool.acceptedFormats[platform]) {
          expect(validFormats.has(f)).toBe(true);
          expect(MANIFEST_FORMATS[f].kind).toBe(tool.acceptsKind);
          expect(MANIFEST_FORMATS[f].platforms[platform].decode).toBe(true);
        }

        expect(tool.outputFormats[platform].length).toBeGreaterThan(0);
        for (const f of tool.outputFormats[platform]) {
          expect(validFormats.has(f)).toBe(true);
          expect(MANIFEST_FORMATS[f].platforms[platform].encode).toBe(true);
        }
      }

      // Options & constraints
      const optionKeys = new Set<string>();
      for (const opt of tool.options) {
        expect(optionKeys.has(opt.key)).toBe(false);
        optionKeys.add(opt.key);
        expect(opt.platforms.length).toBeGreaterThan(0);

        if (opt.type === 'number') {
          expect(opt.min).toBeLessThanOrEqual(opt.max);
          expect(opt.default).toBeGreaterThanOrEqual(opt.min);
          expect(opt.default).toBeLessThanOrEqual(opt.max);
          if (opt.step !== undefined) expect(opt.step).toBeGreaterThan(0);
        } else if (opt.type === 'select') {
          const vals = opt.choices.map((c) => c.value);
          expect(new Set(vals).size).toBe(vals.length);
          expect(vals).toContain(opt.default);
        } else {
          expect(typeof opt.default).toBe('string');
        }
      }

      for (const opt of tool.options) {
        if (opt.showIf) {
          expect(optionKeys.has(opt.showIf.key)).toBe(true);
          expect(opt.showIf.is.length).toBeGreaterThan(0);
        }
      }

      // Runtime TOOLS[id] web options resolve defaults cleanly
      const runtimeTool = TOOLS[tool.id];
      const resolved = resolveOptions(runtimeTool.options);
      for (const opt of runtimeTool.options) {
        expect(resolved[opt.key]).toBe(opt.default);
      }
    }
  });

  it('explicitly models platform-specific formats, options, and external dependencies', () => {
    // HEIC is Web-only decode
    expect(MANIFEST_FORMATS.heic.platforms.web).toMatchObject({ decode: true, encode: false });
    expect(MANIFEST_FORMATS.heic.platforms.desktop).toMatchObject({ decode: false, encode: false });
    for (const id of ['convert-image', 'compress-image', 'resize-image', 'images-to-pdf'] as ToolId[]) {
      expect(TOOL_MANIFEST[id].acceptedFormats.web).toContain('heic' as Format);
      expect(TOOL_MANIFEST[id].acceptedFormats.desktop).not.toContain('heic' as Format);
    }

    // Shared vs Web-only vs Desktop-only options
    expect(manifestOptionsForPlatform('images-to-pdf', 'web').map((o) => o.key)).toEqual(['pageSize']);
    expect(manifestOptionsForPlatform('images-to-pdf', 'desktop').map((o) => o.key)).toEqual(['dpi', 'quality']);
    expect(manifestOptionsForPlatform('compress-image', 'web').map((o) => o.key)).toEqual(['to', 'targetKB']);
    expect(manifestOptionsForPlatform('compress-image', 'desktop').map((o) => o.key)).toEqual(['targetKB']);

    // pdf-to-images DPI: Web keeps the shared 300 ceiling; Desktop's platformBounds raises it to 600.
    const dpiOn = (platform: 'web' | 'desktop') =>
      manifestOptionsForPlatform('pdf-to-images', platform).find((o) => o.key === 'dpi');
    expect(dpiOn('web')).toMatchObject({ min: 36, max: 300 });
    expect(dpiOn('desktop')).toMatchObject({ min: 36, max: 600 });

    // External dependencies on Desktop
    expect(TOOL_MANIFEST['convert-audio'].platformSupport.desktop.status).toBe('requires-dependency');
    expect(TOOL_MANIFEST['convert-audio'].platformSupport.desktop.externalDependencies).toEqual(['ffmpeg', 'ffprobe']);
    expect(TOOL_MANIFEST['trim-audio'].platformSupport.desktop.externalDependencies).toEqual(['ffmpeg', 'ffprobe']);
    expect(TOOL_MANIFEST['pdf-to-images'].platformSupport.desktop.status).toBe('requires-dependency');
    expect(TOOL_MANIFEST['pdf-to-images'].platformSupport.desktop.fallback).toBeTruthy();
  });

  it('agrees with specs/tool-manifest.json and generated Rust manifest', () => {
    expect(MANIFEST_VERSION).toBe(RAW_MANIFEST.manifestVersion);
    expect(RUST_GENERATED).toContain(`pub const MANIFEST_VERSION: u32 = ${MANIFEST_VERSION};`);
    for (const tool of RAW_MANIFEST.tools) {
      expect(TOOL_MANIFEST[tool.id as ToolId].label).toBe(tool.label);
      expect(TOOLS[tool.id as ToolId].label).toBe(tool.label);
      expect(TOOLS[tool.id as ToolId].limits).toBe(tool.limitsKey);
      expect(RUST_GENERATED).toContain(`=> "${tool.id}"`);
    }
  });
});
