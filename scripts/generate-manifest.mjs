#!/usr/bin/env node
/**
 * Deterministic code & documentation generator for specs/tool-manifest.json.
 *
 * Outputs:
 *   - packages/core/src/generated/toolManifest.ts
 *   - crates/refract-core/src/generated_manifest.rs
 *   - docs/platform-matrix.md
 *
 * Usage:
 *   node scripts/generate-manifest.mjs          # validate and write generated files
 *   node scripts/generate-manifest.mjs --check  # validate and fail if any generated file drifted
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT = path.resolve(__dirname, '..');

const MANIFEST_PATH = path.join(ROOT, 'specs', 'tool-manifest.json');
const SCHEMA_PATH = path.join(ROOT, 'specs', 'tool-manifest.schema.json');

const OUT_TS = path.join(ROOT, 'packages', 'core', 'src', 'generated', 'toolManifest.ts');
const OUT_RS = path.join(ROOT, 'crates', 'refract-core', 'src', 'generated_manifest.rs');
const OUT_MD = path.join(ROOT, 'docs', 'platform-matrix.md');

const PLATFORMS = ['web', 'desktop'];
const KINDS = ['image', 'pdf', 'audio'];
const VALID_EXT_RULES = ['fromOptionTo', 'sameOrJpeg', 'sameAudioOrMp3', 'fixed'];
const VALID_STATUSES = ['available', 'requires-dependency', 'unsupported'];
const VALID_DESKTOP_ENGINES = ['builtin', 'ffmpeg', 'pdf-renderer'];

function assert(cond, msg) {
  if (!cond) {
    throw new Error(`Manifest validation failed: ${msg}`);
  }
}

function toPascalCase(id) {
  return id
    .split('-')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join('');
}

function validateManifest(manifest, schema) {
  assert(schema && typeof schema === 'object' && schema.$schema, 'specs/tool-manifest.schema.json is invalid');
  assert(manifest && typeof manifest === 'object', 'manifest root must be an object');
  assert(Number.isInteger(manifest.manifestVersion) && manifest.manifestVersion >= 1, 'manifestVersion must be >= 1');
  assert(manifest.formats && typeof manifest.formats === 'object', 'formats must be an object');
  assert(manifest.limits && typeof manifest.limits === 'object', 'limits must be an object');
  assert(manifest.memoryBudgetMB && typeof manifest.memoryBudgetMB === 'object', 'memoryBudgetMB must be an object');
  assert(Array.isArray(manifest.tools) && manifest.tools.length > 0, 'tools must be a non-empty array');

  for (const p of PLATFORMS) {
    assert(Number.isInteger(manifest.memoryBudgetMB[p]) && manifest.memoryBudgetMB[p] >= 64, `memoryBudgetMB.${p} must be >= 64`);
  }

  // Validate formats
  const formatIds = new Set(Object.keys(manifest.formats));
  for (const [key, fmt] of Object.entries(manifest.formats)) {
    assert(fmt.id === key, `format key "${key}" must match format.id "${fmt.id}"`);
    assert(KINDS.includes(fmt.kind), `format "${key}" has invalid kind "${fmt.kind}"`);
    assert(typeof fmt.label === 'string' && fmt.label.length > 0, `format "${key}" missing label`);
    assert(typeof fmt.displayName === 'string' && fmt.displayName.length > 0, `format "${key}" missing displayName`);
    assert(Array.isArray(fmt.extensions) && fmt.extensions.length > 0, `format "${key}" missing extensions`);
    assert(typeof fmt.mime === 'string' && fmt.mime.includes('/'), `format "${key}" has invalid mime`);
    for (const p of PLATFORMS) {
      assert(fmt.platforms?.[p] && typeof fmt.platforms[p].decode === 'boolean' && typeof fmt.platforms[p].encode === 'boolean', `format "${key}" missing platforms.${p} decode/encode booleans`);
    }
  }

  // Validate limits
  const limitKeys = new Set(Object.keys(manifest.limits));
  for (const [key, entry] of Object.entries(manifest.limits)) {
    for (const p of PLATFORMS) {
      const lim = entry[p];
      assert(lim && Number.isInteger(lim.maxMB) && lim.maxMB >= 1, `limits.${key}.${p}.maxMB invalid`);
      assert(Number.isInteger(lim.maxFiles) && lim.maxFiles >= 1, `limits.${key}.${p}.maxFiles invalid`);
      if (lim.maxPages !== undefined) {
        assert(Number.isInteger(lim.maxPages) && lim.maxPages >= 1, `limits.${key}.${p}.maxPages invalid`);
      }
      if (lim.maxPixels !== undefined) {
        assert(Number.isInteger(lim.maxPixels) && lim.maxPixels >= 1, `limits.${key}.${p}.maxPixels invalid`);
      }
    }
  }

  // Validate tools
  const seenToolIds = new Set();
  for (const tool of manifest.tools) {
    assert(typeof tool.id === 'string' && /^[a-z]+(-[a-z]+)+$/.test(tool.id), `invalid tool id "${tool.id}"`);
    assert(!seenToolIds.has(tool.id), `duplicate tool id "${tool.id}"`);
    seenToolIds.add(tool.id);

    assert(KINDS.includes(tool.domain), `tool "${tool.id}" has invalid domain "${tool.domain}"`);
    assert(KINDS.includes(tool.acceptsKind), `tool "${tool.id}" has invalid acceptsKind "${tool.acceptsKind}"`);
    assert(typeof tool.label === 'string' && tool.label.length > 0, `tool "${tool.id}" missing label`);
    assert(typeof tool.blurb === 'string' && tool.blurb.length > 0, `tool "${tool.id}" missing blurb`);
    assert(typeof tool.tagline === 'string' && tool.tagline.length > 0, `tool "${tool.id}" missing tagline`);
    assert(typeof tool.description === 'string' && tool.description.length > 0, `tool "${tool.id}" missing description`);
    assert(typeof tool.outputCapability === 'string' && tool.outputCapability.length > 0, `tool "${tool.id}" missing outputCapability`);
    assert(Array.isArray(tool.searchKeywords) && tool.searchKeywords.length > 0, `tool "${tool.id}" missing searchKeywords`);
    assert(typeof tool.popular === 'boolean', `tool "${tool.id}" popular must be boolean`);

    // Inputs
    assert(tool.inputs && (tool.inputs.mode === 'one' || tool.inputs.mode === 'many'), `tool "${tool.id}" invalid inputs.mode`);
    assert(Number.isInteger(tool.inputs.min) && tool.inputs.min >= 1, `tool "${tool.id}" inputs.min must be >= 1`);
    assert(tool.inputs.max === null || (Number.isInteger(tool.inputs.max) && tool.inputs.max >= tool.inputs.min), `tool "${tool.id}" inputs.max invalid`);

    // Formats + platform decode/encode consistency
    for (const p of PLATFORMS) {
      const acc = tool.acceptedFormats?.[p];
      assert(Array.isArray(acc) && acc.length > 0, `tool "${tool.id}" missing acceptedFormats.${p}`);
      assert(new Set(acc).size === acc.length, `tool "${tool.id}" duplicate acceptedFormats.${p}`);
      for (const f of acc) {
        assert(formatIds.has(f), `tool "${tool.id}" unknown accepted format "${f}"`);
        assert(manifest.formats[f].kind === tool.acceptsKind, `tool "${tool.id}" accepted format "${f}" does not match acceptsKind "${tool.acceptsKind}"`);
        assert(manifest.formats[f].platforms[p].decode === true, `tool "${tool.id}" accepts "${f}" on ${p}, but formats.${f}.platforms.${p}.decode is false`);
      }

      const out = tool.outputFormats?.[p];
      assert(Array.isArray(out) && out.length > 0, `tool "${tool.id}" missing outputFormats.${p}`);
      assert(new Set(out).size === out.length, `tool "${tool.id}" duplicate outputFormats.${p}`);
      for (const f of out) {
        assert(formatIds.has(f), `tool "${tool.id}" unknown output format "${f}"`);
        assert(manifest.formats[f].platforms[p].encode === true, `tool "${tool.id}" outputs "${f}" on ${p}, but formats.${f}.platforms.${p}.encode is false`);
      }
    }

    // Limits
    assert(limitKeys.has(tool.limitsKey), `tool "${tool.id}" unknown limitsKey "${tool.limitsKey}"`);

    // Output naming
    assert(tool.outputNaming && typeof tool.outputNaming.template === 'string' && tool.outputNaming.template.length > 0, `tool "${tool.id}" missing outputNaming.template`);
    assert(typeof tool.outputNaming.multi === 'boolean', `tool "${tool.id}" outputNaming.multi must be boolean`);
    assert(VALID_EXT_RULES.includes(tool.outputNaming.extRule), `tool "${tool.id}" invalid outputNaming.extRule "${tool.outputNaming.extRule}"`);
    if (tool.outputNaming.extRule === 'fixed') {
      assert(typeof tool.outputNaming.fixedExt === 'string' && tool.outputNaming.fixedExt.length > 0, `tool "${tool.id}" fixed extRule requires fixedExt`);
    }

    // Capabilities & platform support
    assert(tool.capabilities && typeof tool.capabilities.batch === 'boolean' && typeof tool.capabilities.preview === 'boolean' && typeof tool.capabilities.loupe === 'boolean' && typeof tool.capabilities.chainOutput === 'boolean', `tool "${tool.id}" invalid capabilities`);
    assert(typeof tool.capabilities.passwordUnlock?.web === 'boolean' && typeof tool.capabilities.passwordUnlock?.desktop === 'boolean', `tool "${tool.id}" invalid passwordUnlock capabilities`);
    assert(VALID_STATUSES.includes(tool.platformSupport?.web?.status), `tool "${tool.id}" invalid web status`);
    assert(typeof tool.platformSupport?.web?.engine === 'string' && tool.platformSupport.web.engine.length > 0, `tool "${tool.id}" missing web engine`);
    assert(VALID_STATUSES.includes(tool.platformSupport?.desktop?.status), `tool "${tool.id}" invalid desktop status`);
    assert(VALID_DESKTOP_ENGINES.includes(tool.platformSupport?.desktop?.engine), `tool "${tool.id}" invalid desktop engine`);
    assert(Array.isArray(tool.platformSupport?.desktop?.externalDependencies), `tool "${tool.id}" missing desktop externalDependencies`);

    // Options
    assert(Array.isArray(tool.options), `tool "${tool.id}" options must be an array`);
    const optionKeys = new Set();
    for (const opt of tool.options) {
      assert(typeof opt.key === 'string' && /^[a-zA-Z][a-zA-Z0-9_]*$/.test(opt.key), `tool "${tool.id}" invalid option key "${opt.key}"`);
      assert(!optionKeys.has(opt.key), `tool "${tool.id}" duplicate option key "${opt.key}"`);
      optionKeys.add(opt.key);

      assert(Array.isArray(opt.platforms) && opt.platforms.length > 0 && opt.platforms.every((p) => PLATFORMS.includes(p)), `tool "${tool.id}" option "${opt.key}" invalid platforms`);
      assert(['select', 'number', 'text'].includes(opt.type), `tool "${tool.id}" option "${opt.key}" invalid type "${opt.type}"`);

      if (opt.type === 'number') {
        assert(typeof opt.min === 'number' && typeof opt.max === 'number' && opt.min <= opt.max, `tool "${tool.id}" option "${opt.key}" invalid min/max`);
        assert(typeof opt.default === 'number' && opt.default >= opt.min && opt.default <= opt.max, `tool "${tool.id}" option "${opt.key}" default ${opt.default} outside [${opt.min}, ${opt.max}]`);
        if (opt.step !== undefined) {
          assert(typeof opt.step === 'number' && opt.step > 0, `tool "${tool.id}" option "${opt.key}" step must be > 0`);
        }
      } else if (opt.type === 'select') {
        assert(Array.isArray(opt.choices) && opt.choices.length > 0, `tool "${tool.id}" option "${opt.key}" select missing choices`);
        const choiceValues = new Set(opt.choices.map((c) => c.value));
        assert(choiceValues.size === opt.choices.length, `tool "${tool.id}" option "${opt.key}" duplicate choices`);
        assert(typeof opt.default === 'string' && choiceValues.has(opt.default), `tool "${tool.id}" option "${opt.key}" default "${opt.default}" not in choices`);
      } else if (opt.type === 'text') {
        assert(typeof opt.default === 'string', `tool "${tool.id}" option "${opt.key}" text default must be string`);
      }
    }

    for (const opt of tool.options) {
      if (opt.showIf) {
        assert(optionKeys.has(opt.showIf.key), `tool "${tool.id}" option "${opt.key}" showIf references unknown key "${opt.showIf.key}"`);
        assert(Array.isArray(opt.showIf.is) && opt.showIf.is.length > 0, `tool "${tool.id}" option "${opt.key}" showIf.is must be non-empty`);
      }
    }
  }
}

function generateTypeScript(manifest) {
  const formatIdUnion = Object.keys(manifest.formats).map((id) => `'${id}'`).join(' | ');
  const toolIdUnion = manifest.tools.map((t) => `'${t.id}'`).join(' | ');
  const limitKeyUnion = Object.keys(manifest.limits).map((k) => `'${k}'`).join(' | ');

  const formatsRecord = Object.fromEntries(
    Object.entries(manifest.formats).map(([id, f]) => [
      id,
      { label: f.label, ext: f.extensions, mime: f.mime, kind: f.kind },
    ]),
  );

  const toolsRecord = Object.fromEntries(manifest.tools.map((t) => [t.id, t]));

  const lines = [
    '// GENERATED FILE — DO NOT EDIT BY HAND.',
    '// Source: specs/tool-manifest.json',
    '// Regenerate with: pnpm manifest:generate',
    '',
    `export const MANIFEST_VERSION = ${manifest.manifestVersion} as const;`,
    '',
    `export type Format = ${formatIdUnion};`,
    `export type Kind = 'image' | 'pdf' | 'audio';`,
    `export type Platform = 'web' | 'desktop';`,
    `export type ToolId = ${toolIdUnion};`,
    `export type LimitKey = ${limitKeyUnion};`,
    `export type PlatformAvailabilityStatus = 'available' | 'requires-dependency' | 'unsupported';`,
    `export type DesktopEngineKind = 'builtin' | 'ffmpeg' | 'pdf-renderer';`,
    `export type OutputExtRule = 'fromOptionTo' | 'sameOrJpeg' | 'sameAudioOrMp3' | 'fixed';`,
    '',
    'export type FormatPlatformSupport = {',
    '  decode: boolean;',
    '  encode: boolean;',
    '  note?: string;',
    '};',
    '',
    'export type ManifestFormatSpec = {',
    '  id: Format;',
    '  label: string;',
    '  displayName: string;',
    '  kind: Kind;',
    '  extensions: readonly string[];',
    '  mime: string;',
    '  platforms: Record<Platform, FormatPlatformSupport>;',
    '};',
    '',
    'export type Limits = { maxMB: number; maxFiles: number; maxPages?: number; maxPixels?: number };',
    '',
    'export type ManifestOptionChoice = {',
    '  value: string;',
    '  label: string;',
    '  platforms?: readonly Platform[];',
    '};',
    '',
    'type ManifestOptionBase = {',
    '  key: string;',
    '  label: string;',
    '  platforms: readonly Platform[];',
    '  help?: string;',
    '  showIf?: { key: string; is: readonly (string | number)[] };',
    '};',
    '',
    'export type ManifestOptionSpec =',
    "  | (ManifestOptionBase & { type: 'number'; min: number; max: number; step?: number; unit?: string; default: number })",
    "  | (ManifestOptionBase & { type: 'select'; choices: readonly ManifestOptionChoice[]; default: string })",
    "  | (ManifestOptionBase & { type: 'text'; placeholder?: string; default: string });",
    '',
    'export type ManifestToolSpec = {',
    '  id: ToolId;',
    '  domain: Kind;',
    '  acceptsKind: Kind;',
    '  label: string;',
    '  blurb: string;',
    '  tagline: string;',
    '  description: string;',
    '  outputCapability: string;',
    '  searchKeywords: readonly string[];',
    '  popular: boolean;',
    "  inputs: { mode: 'one' | 'many'; min: number; max: number | null };",
    '  acceptedFormats: Record<Platform, readonly Format[]>;',
    '  outputFormats: Record<Platform, readonly Format[]>;',
    '  options: readonly ManifestOptionSpec[];',
    '  outputNaming: {',
    '    template: string;',
    '    multi: boolean;',
    '    extRule: OutputExtRule;',
    '    fixedExt?: string;',
    '    entryTemplate?: string;',
    '  };',
    '  limitsKey: LimitKey;',
    '  capabilities: {',
    '    batch: boolean;',
    '    preview: boolean;',
    '    loupe: boolean;',
    '    passwordUnlock: Record<Platform, boolean>;',
    '    chainOutput: boolean;',
    '  };',
    '  platformSupport: {',
    '    web: { status: PlatformAvailabilityStatus; engine: string; lazyLoaded?: boolean; note?: string };',
    '    desktop: {',
    '      status: PlatformAvailabilityStatus;',
    '      engine: DesktopEngineKind;',
    '      engineLabel: string;',
    '      externalDependencies: readonly string[];',
    '      fallback?: string;',
    '      note?: string;',
    '    };',
    '  };',
    '};',
    '',
    `export const TOOL_IDS: readonly ToolId[] = ${JSON.stringify(manifest.tools.map((t) => t.id), null, 2)} as const;`,
    '',
    `export const MANIFEST_FORMATS: Record<Format, ManifestFormatSpec> = ${JSON.stringify(manifest.formats, null, 2)};`,
    '',
    `export const FORMATS: Record<Format, { label: string; ext: string[]; mime: string; kind: Kind }> = ${JSON.stringify(formatsRecord, null, 2)};`,
    '',
    `export const LIMITS: Record<string, Record<Platform, Limits>> = ${JSON.stringify(manifest.limits, null, 2)};`,
    '',
    'const MiB = 1024 * 1024;',
    `export const MEMORY_BUDGET_BYTES: Record<Platform, number> = { web: ${manifest.memoryBudgetMB.web} * MiB, desktop: ${manifest.memoryBudgetMB.desktop} * MiB };`,
    '',
    `export const TOOL_MANIFEST: Record<ToolId, ManifestToolSpec> = ${JSON.stringify(toolsRecord, null, 2)};`,
    '',
    `export const TOOL_MANIFEST_LIST: readonly ManifestToolSpec[] = TOOL_IDS.map((id) => TOOL_MANIFEST[id]);`,
    '',
    '/** Filter a tool\'s manifest options for a specific platform (`web` or `desktop`). */',
    'export function manifestOptionsForPlatform(id: ToolId, platform: Platform): ManifestOptionSpec[] {',
    '  return TOOL_MANIFEST[id].options',
    '    .filter((opt) => opt.platforms.includes(platform))',
    '    .map((opt) => {',
    "      if (opt.type === 'select') {",
    '        return {',
    '          ...opt,',
    '          choices: opt.choices.filter((c) => !c.platforms || c.platforms.includes(platform)),',
    '        };',
    '      }',
    '      return opt;',
    '    });',
    '}',
    '',
  ];

  return lines.join('\n');
}

function rsStr(s) {
  return JSON.stringify(s);
}

function rsOptStr(s) {
  return s === undefined || s === null ? 'None' : `Some(${rsStr(s)})`;
}

function rsKind(k) {
  return k === 'image' ? 'Kind::Image' : k === 'pdf' ? 'Kind::Pdf' : 'Kind::Audio';
}

function rsFormat(f) {
  const map = {
    heic: 'Format::Heic',
    jpeg: 'Format::Jpeg',
    png: 'Format::Png',
    webp: 'Format::Webp',
    pdf: 'Format::Pdf',
    mp3: 'Format::Mp3',
    wav: 'Format::Wav',
    m4a: 'Format::M4a',
    ogg: 'Format::Ogg',
  };
  return map[f];
}

function rsEngine(e) {
  if (e === 'builtin') return 'ToolEngine::Builtin';
  if (e === 'ffmpeg') return 'ToolEngine::Ffmpeg';
  return 'ToolEngine::PdfRenderer';
}

function rsAvailability(s) {
  if (s === 'available') return 'PlatformAvailability::Available';
  if (s === 'requires-dependency') return 'PlatformAvailability::RequiresDependency';
  return 'PlatformAvailability::Unsupported';
}

function rsPlatform(p) {
  return p === 'web' ? 'Platform::Web' : 'Platform::Desktop';
}

function rsFloat(n) {
  const s = String(n);
  return s.includes('.') ? s : `${s}.0`;
}

function generateRust(manifest) {
  const lines = [
    '// GENERATED FILE — DO NOT EDIT BY HAND.',
    '// Source: specs/tool-manifest.json',
    '// Regenerate with: pnpm manifest:generate',
    '',
    'use crate::format::{Format, Kind};',
    'use serde::{Deserialize, Serialize};',
    '',
    `pub const MANIFEST_VERSION: u32 = ${manifest.manifestVersion};`,
    `pub const WEB_MEMORY_BUDGET_MB: u64 = ${manifest.memoryBudgetMB.web};`,
    `pub const DESKTOP_MEMORY_BUDGET_MB: u64 = ${manifest.memoryBudgetMB.desktop};`,
    '',
    '#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]',
    'pub enum Platform {',
    '    Web,',
    '    Desktop,',
    '}',
    '',
    '#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]',
    'pub enum PlatformAvailability {',
    '    Available,',
    '    RequiresDependency,',
    '    Unsupported,',
    '}',
    '',
    '/// Which native engine backs a tool on desktop.',
    '#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]',
    'pub enum ToolEngine {',
    '    /// Pure Rust, no external dependency.',
    '    Builtin,',
    '    /// Requires `ffmpeg` / `ffprobe` on PATH.',
    '    Ffmpeg,',
    '    /// Requires a PDF rasterizer (`mutool`, `pdftoppm`, `pdftocairo` or `gs`).',
    '    PdfRenderer,',
    '}',
    '',
    'impl ToolEngine {',
    '    pub fn label(&self) -> &\'static str {',
    '        match self {',
    '            ToolEngine::Builtin => "Built-in engine",',
    '            ToolEngine::Ffmpeg => "FFmpeg",',
    '            ToolEngine::PdfRenderer => "PDF renderer",',
    '        }',
    '    }',
    '',
    '    /// Human readable name of the binary that has to be present.',
    '    pub fn binary_hint(&self) -> &\'static str {',
    '        match self {',
    '            ToolEngine::Builtin => "",',
    '            ToolEngine::Ffmpeg => "ffmpeg",',
    '            ToolEngine::PdfRenderer => "mutool / pdftoppm / pdftocairo / gs",',
    '        }',
    '    }',
    '}',
    '',
    '#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]',
    'pub enum ToolId {',
  ];

  for (const t of manifest.tools) {
    lines.push(`    ${toPascalCase(t.id)},`);
  }
  lines.push('}');
  lines.push('');

  lines.push('impl ToolId {');
  lines.push('    pub const ALL: &\'static [ToolId] = &[');
  for (const t of manifest.tools) {
    lines.push(`        ToolId::${toPascalCase(t.id)},`);
  }
  lines.push('    ];');
  lines.push('');
  lines.push('    pub fn id_str(&self) -> &\'static str {');
  lines.push('        match self {');
  for (const t of manifest.tools) {
    lines.push(`            ToolId::${toPascalCase(t.id)} => ${rsStr(t.id)},`);
  }
  lines.push('        }');
  lines.push('    }');
  lines.push('');
  lines.push('    pub fn from_id_str(id: &str) -> Option<ToolId> {');
  lines.push('        match id {');
  for (const t of manifest.tools) {
    lines.push(`            ${rsStr(t.id)} => Some(ToolId::${toPascalCase(t.id)}),`);
  }
  lines.push('            _ => None,');
  lines.push('        }');
  lines.push('    }');
  lines.push('}');
  lines.push('');

  lines.push('#[derive(Debug, Clone, Copy, PartialEq, Eq)]');
  lines.push('pub enum OptionType {');
  lines.push('    Select,');
  lines.push('    Number,');
  lines.push('    Text,');
  lines.push('}');
  lines.push('');

  lines.push('#[derive(Debug, Clone, Copy, PartialEq)]');
  lines.push('pub enum OptionDefault {');
  lines.push('    Text(&\'static str),');
  lines.push('    Number(f64),');
  lines.push('}');
  lines.push('');

  lines.push('#[derive(Debug, Clone, Copy, PartialEq, Eq)]');
  lines.push('pub struct OptionChoice {');
  lines.push('    pub value: &\'static str,');
  lines.push('    pub label: &\'static str,');
  lines.push('}');
  lines.push('');

  lines.push('#[derive(Debug, Clone, Copy, PartialEq, Eq)]');
  lines.push('pub struct OptionShowIf {');
  lines.push('    pub key: &\'static str,');
  lines.push('    pub values: &\'static [&\'static str],');
  lines.push('}');
  lines.push('');

  lines.push('#[derive(Debug, Clone, Copy, PartialEq)]');
  lines.push('pub struct ManifestOptionSpec {');
  lines.push('    pub key: &\'static str,');
  lines.push('    pub label: &\'static str,');
  lines.push('    pub option_type: OptionType,');
  lines.push('    pub platforms: &\'static [Platform],');
  lines.push('    pub default: OptionDefault,');
  lines.push('    pub min: Option<f64>,');
  lines.push('    pub max: Option<f64>,');
  lines.push('    pub step: Option<f64>,');
  lines.push('    pub unit: Option<&\'static str>,');
  lines.push('    pub placeholder: Option<&\'static str>,');
  lines.push('    pub help: Option<&\'static str>,');
  lines.push('    pub choices: &\'static [OptionChoice],');
  lines.push('    pub show_if: Option<OptionShowIf>,');
  lines.push('}');
  lines.push('');

  lines.push('#[derive(Debug, Clone, Copy, PartialEq, Eq)]');
  lines.push('pub struct LimitsSpec {');
  lines.push('    pub key: &\'static str,');
  lines.push('    pub web_max_mb: u64,');
  lines.push('    pub web_max_files: usize,');
  lines.push('    pub web_max_pages: Option<u32>,');
  lines.push('    pub web_max_pixels: Option<u64>,');
  lines.push('    pub desktop_max_mb: u64,');
  lines.push('    pub desktop_max_files: usize,');
  lines.push('    pub desktop_max_pages: Option<u32>,');
  lines.push('    pub desktop_max_pixels: Option<u64>,');
  lines.push('}');
  lines.push('');

  lines.push('#[derive(Debug, Clone, Copy, PartialEq, Eq)]');
  lines.push('pub struct FormatSupportEntry {');
  lines.push('    pub format: Format,');
  lines.push('    pub web_decode: bool,');
  lines.push('    pub web_encode: bool,');
  lines.push('    pub desktop_decode: bool,');
  lines.push('    pub desktop_encode: bool,');
  lines.push('}');
  lines.push('');

  lines.push('#[derive(Debug, Clone, Copy)]');
  lines.push('pub struct ToolManifestEntry {');
  lines.push('    pub id: ToolId,');
  lines.push('    pub domain: Kind,');
  lines.push('    pub accepts_kind: Kind,');
  lines.push('    pub label: &\'static str,');
  lines.push('    pub blurb: &\'static str,');
  lines.push('    pub tagline: &\'static str,');
  lines.push('    pub description: &\'static str,');
  lines.push('    pub output_capability: &\'static str,');
  lines.push('    pub batch: bool,');
  lines.push('    pub min_files: usize,');
  lines.push('    pub max_files: Option<usize>,');
  lines.push('    pub web_accepted_formats: &\'static [Format],');
  lines.push('    pub desktop_accepted_formats: &\'static [Format],');
  lines.push('    pub web_output_formats: &\'static [Format],');
  lines.push('    pub desktop_output_formats: &\'static [Format],');
  lines.push('    pub desktop_input_format_labels: &\'static [&\'static str],');
  lines.push('    pub desktop_extensions: &\'static [&\'static str],');
  lines.push('    pub output_template: &\'static str,');
  lines.push('    pub output_multi: bool,');
  lines.push('    pub limits_key: &\'static str,');
  lines.push('    pub desktop_engine: ToolEngine,');
  lines.push('    pub desktop_availability: PlatformAvailability,');
  lines.push('    pub desktop_external_dependencies: &\'static [&\'static str],');
  lines.push('    pub web_password_unlock: bool,');
  lines.push('    pub desktop_password_unlock: bool,');
  lines.push('    pub options: &\'static [ManifestOptionSpec],');
  lines.push('}');
  lines.push('');

  // FORMAT_SUPPORT
  lines.push('pub const FORMAT_SUPPORT: &[FormatSupportEntry] = &[');
  for (const [id, f] of Object.entries(manifest.formats)) {
    lines.push('    FormatSupportEntry {');
    lines.push(`        format: ${rsFormat(id)},`);
    lines.push(`        web_decode: ${f.platforms.web.decode},`);
    lines.push(`        web_encode: ${f.platforms.web.encode},`);
    lines.push(`        desktop_decode: ${f.platforms.desktop.decode},`);
    lines.push(`        desktop_encode: ${f.platforms.desktop.encode},`);
    lines.push('    },');
  }
  lines.push('];');
  lines.push('');

  // LIMITS_MANIFEST
  lines.push('pub const LIMITS_MANIFEST: &[LimitsSpec] = &[');
  for (const [key, entry] of Object.entries(manifest.limits)) {
    lines.push('    LimitsSpec {');
    lines.push(`        key: ${rsStr(key)},`);
    lines.push(`        web_max_mb: ${entry.web.maxMB},`);
    lines.push(`        web_max_files: ${entry.web.maxFiles},`);
    lines.push(`        web_max_pages: ${entry.web.maxPages !== undefined ? `Some(${entry.web.maxPages})` : 'None'},`);
    lines.push(`        web_max_pixels: ${entry.web.maxPixels !== undefined ? `Some(${entry.web.maxPixels})` : 'None'},`);
    lines.push(`        desktop_max_mb: ${entry.desktop.maxMB},`);
    lines.push(`        desktop_max_files: ${entry.desktop.maxFiles},`);
    lines.push(`        desktop_max_pages: ${entry.desktop.maxPages !== undefined ? `Some(${entry.desktop.maxPages})` : 'None'},`);
    lines.push(`        desktop_max_pixels: ${entry.desktop.maxPixels !== undefined ? `Some(${entry.desktop.maxPixels})` : 'None'},`);
    lines.push('    },');
  }
  lines.push('];');
  lines.push('');

  // TOOL_MANIFEST
  lines.push('pub const TOOL_MANIFEST: &[ToolManifestEntry] = &[');
  for (const t of manifest.tools) {
    const desktopLabels = t.acceptedFormats.desktop.map((f) => manifest.formats[f].displayName);
    const desktopExts = [];
    for (const f of t.acceptedFormats.desktop) {
      for (const e of manifest.formats[f].extensions) {
        if (!desktopExts.includes(e)) desktopExts.push(e);
      }
    }

    lines.push('    ToolManifestEntry {');
    lines.push(`        id: ToolId::${toPascalCase(t.id)},`);
    lines.push(`        domain: ${rsKind(t.domain)},`);
    lines.push(`        accepts_kind: ${rsKind(t.acceptsKind)},`);
    lines.push(`        label: ${rsStr(t.label)},`);
    lines.push(`        blurb: ${rsStr(t.blurb)},`);
    lines.push(`        tagline: ${rsStr(t.tagline)},`);
    lines.push(`        description: ${rsStr(t.description)},`);
    lines.push(`        output_capability: ${rsStr(t.outputCapability)},`);
    lines.push(`        batch: ${t.capabilities.batch},`);
    lines.push(`        min_files: ${t.inputs.min},`);
    lines.push(`        max_files: ${t.inputs.max === null ? 'None' : `Some(${t.inputs.max})`},`);
    lines.push(`        web_accepted_formats: &[${t.acceptedFormats.web.map(rsFormat).join(', ')}],`);
    lines.push(`        desktop_accepted_formats: &[${t.acceptedFormats.desktop.map(rsFormat).join(', ')}],`);
    lines.push(`        web_output_formats: &[${t.outputFormats.web.map(rsFormat).join(', ')}],`);
    lines.push(`        desktop_output_formats: &[${t.outputFormats.desktop.map(rsFormat).join(', ')}],`);
    lines.push(`        desktop_input_format_labels: &[${desktopLabels.map(rsStr).join(', ')}],`);
    lines.push(`        desktop_extensions: &[${desktopExts.map(rsStr).join(', ')}],`);
    lines.push(`        output_template: ${rsStr(t.outputNaming.template)},`);
    lines.push(`        output_multi: ${t.outputNaming.multi},`);
    lines.push(`        limits_key: ${rsStr(t.limitsKey)},`);
    lines.push(`        desktop_engine: ${rsEngine(t.platformSupport.desktop.engine)},`);
    lines.push(`        desktop_availability: ${rsAvailability(t.platformSupport.desktop.status)},`);
    lines.push(`        desktop_external_dependencies: &[${t.platformSupport.desktop.externalDependencies.map(rsStr).join(', ')}],`);
    lines.push(`        web_password_unlock: ${t.capabilities.passwordUnlock.web},`);
    lines.push(`        desktop_password_unlock: ${t.capabilities.passwordUnlock.desktop},`);

    if (t.options.length === 0) {
      lines.push('        options: &[],');
    } else {
      lines.push('        options: &[');
      for (const opt of t.options) {
        const optType =
          opt.type === 'select' ? 'OptionType::Select'
            : opt.type === 'number' ? 'OptionType::Number'
              : 'OptionType::Text';
        const defVal =
          typeof opt.default === 'number'
            ? `OptionDefault::Number(${rsFloat(opt.default)})`
            : `OptionDefault::Text(${rsStr(opt.default)})`;

        lines.push('            ManifestOptionSpec {');
        lines.push(`                key: ${rsStr(opt.key)},`);
        lines.push(`                label: ${rsStr(opt.label)},`);
        lines.push(`                option_type: ${optType},`);
        lines.push(`                platforms: &[${opt.platforms.map(rsPlatform).join(', ')}],`);
        lines.push(`                default: ${defVal},`);
        lines.push(`                min: ${opt.min !== undefined ? `Some(${rsFloat(opt.min)})` : 'None'},`);
        lines.push(`                max: ${opt.max !== undefined ? `Some(${rsFloat(opt.max)})` : 'None'},`);
        lines.push(`                step: ${opt.step !== undefined ? `Some(${rsFloat(opt.step)})` : 'None'},`);
        lines.push(`                unit: ${rsOptStr(opt.unit)},`);
        lines.push(`                placeholder: ${rsOptStr(opt.placeholder)},`);
        lines.push(`                help: ${rsOptStr(opt.help)},`);
        if (opt.choices && opt.choices.length > 0) {
          lines.push('                choices: &[');
          for (const c of opt.choices) {
            lines.push('                    OptionChoice {');
            lines.push(`                        value: ${rsStr(c.value)},`);
            lines.push(`                        label: ${rsStr(c.label)},`);
            lines.push('                    },');
          }
          lines.push('                ],');
        } else {
          lines.push('                choices: &[],');
        }
        if (opt.showIf) {
          const vals = opt.showIf.is.map((v) => rsStr(String(v))).join(', ');
          lines.push('                show_if: Some(OptionShowIf {');
          lines.push(`                    key: ${rsStr(opt.showIf.key)},`);
          lines.push(`                    values: &[${vals}],`);
          lines.push('                }),');
        } else {
          lines.push('                show_if: None,');
        }
        lines.push('            },');
      }
      lines.push('        ],');
    }
    lines.push('    },');
  }
  lines.push('];');
  lines.push('');

  lines.push('pub fn manifest_for(id: ToolId) -> &\'static ToolManifestEntry {');
  lines.push('    TOOL_MANIFEST');
  lines.push('        .iter()');
  lines.push('        .find(|entry| entry.id == id)');
  lines.push('        .expect("Every ToolId has a generated manifest entry")');
  lines.push('}');
  lines.push('');

  lines.push('pub fn limits_for_key(key: &str) -> Option<&\'static LimitsSpec> {');
  lines.push('    LIMITS_MANIFEST.iter().find(|entry| entry.key == key)');
  lines.push('}');
  lines.push('');

  return lines.join('\n');
}

function generatePlatformMatrixMarkdown(manifest) {
  const fmtList = (arr) => arr.map((f) => manifest.formats[f].displayName).join(', ');
  const yesNo = (b) => (b ? 'Yes' : 'No');
  const statusBadge = (s) =>
    s === 'available' ? 'Available (built-in)'
      : s === 'requires-dependency' ? 'Requires external binary'
        : 'Unsupported';

  const lines = [
    '<!-- GENERATED FILE — DO NOT EDIT BY HAND. -->',
    '<!-- Source: specs/tool-manifest.json -->',
    '<!-- Regenerate with: pnpm manifest:generate -->',
    '',
    '# Refract Platform Support & Capabilities Matrix',
    '',
    `Generated from \`specs/tool-manifest.json\` (manifest version \`${manifest.manifestVersion}\`).`,
    '',
    `Refract ships **${manifest.tools.length} canonical tools** across **3 domains** (Images, PDF, Audio) on both **Refract Web** (WebAssembly in dedicated Web Workers) and **Refract Desktop** (native Rust + Slint UI).`,
    '',
    '---',
    '',
    '## 1. Format Support',
    '',
    '| Format | Kind | Extensions | Web (Decode / Encode) | Desktop (Decode / Encode) | Notes |',
    '| --- | --- | --- | --- | --- | --- |',
  ];

  for (const fmt of Object.values(manifest.formats)) {
    const notes = [fmt.platforms.web.note, fmt.platforms.desktop.note].filter(Boolean).join(' ');
    lines.push(
      `| **${fmt.displayName}** | ${fmt.kind} | \`${fmt.extensions.join('`, `')}\` | ${yesNo(fmt.platforms.web.decode)} / ${yesNo(fmt.platforms.web.encode)} | ${yesNo(fmt.platforms.desktop.decode)} / ${yesNo(fmt.platforms.desktop.encode)} | ${notes || '—'} |`,
    );
  }

  lines.push('');
  lines.push('---');
  lines.push('');
  lines.push('## 2. Tool Support & External Dependencies');
  lines.push('');
  lines.push('| Tool ID | Domain | Inputs (Web / Desktop) | Outputs (Web / Desktop) | Web Engine | Desktop Engine & Status | External Dependencies | Password Unlock (Web / Desktop) |');
  lines.push('| --- | --- | --- | --- | --- | --- | --- | --- |');

  for (const t of manifest.tools) {
    const webIn = fmtList(t.acceptedFormats.web);
    const deskIn = fmtList(t.acceptedFormats.desktop);
    const inCol = webIn === deskIn ? webIn : `Web: ${webIn}<br>Desktop: ${deskIn}`;

    const webOut = fmtList(t.outputFormats.web);
    const deskOut = fmtList(t.outputFormats.desktop);
    const outCol = webOut === deskOut ? webOut : `Web: ${webOut}<br>Desktop: ${deskOut}`;

    const webEng = t.platformSupport.web.engine;
    const deskStatus = `${t.platformSupport.desktop.engineLabel} — ${statusBadge(t.platformSupport.desktop.status)}`;
    const deps = t.platformSupport.desktop.externalDependencies.length
      ? `\`${t.platformSupport.desktop.externalDependencies.join('`, `')}\`${t.platformSupport.desktop.fallback ? ` *(fallback: ${t.platformSupport.desktop.fallback})*` : ''}`
      : 'None';
    const pw = `${yesNo(t.capabilities.passwordUnlock.web)} / ${yesNo(t.capabilities.passwordUnlock.desktop)}`;

    lines.push(`| \`${t.id}\` | ${t.domain} | ${inCol} | ${outCol} | ${webEng} | ${deskStatus} | ${deps} | ${pw} |`);
  }

  lines.push('');
  lines.push('---');
  lines.push('');
  lines.push('## 3. Option Scope (Shared vs. Platform-Specific)');
  lines.push('');
  lines.push('| Tool ID | Shared Options | Web-Only Options | Desktop-Only Options | Canonical Output Template |');
  lines.push('| --- | --- | --- | --- | --- |');

  for (const t of manifest.tools) {
    const shared = t.options.filter((o) => o.platforms.includes('web') && o.platforms.includes('desktop')).map((o) => `\`${o.key}\``);
    const webOnly = t.options.filter((o) => o.platforms.includes('web') && !o.platforms.includes('desktop')).map((o) => `\`${o.key}\``);
    const deskOnly = t.options.filter((o) => !o.platforms.includes('web') && o.platforms.includes('desktop')).map((o) => `\`${o.key}\``);
    lines.push(
      `| \`${t.id}\` | ${shared.length ? shared.join(', ') : '—'} | ${webOnly.length ? webOnly.join(', ') : '—'} | ${deskOnly.length ? deskOnly.join(', ') : '—'} | \`${t.outputNaming.template}\` |`,
    );
  }

  lines.push('');
  lines.push('---');
  lines.push('');
  lines.push('## 4. Resource Limits & Memory Budgets');
  lines.push('');
  lines.push('| Limit Profile | Web Limits | Desktop Limits |');
  lines.push('| --- | --- | --- |');

  const formatLimit = (l) => {
    const parts = [`${l.maxMB} MB/file`, `${l.maxFiles} files`];
    if (l.maxPixels) parts.push(`${Math.round(l.maxPixels / 1e6)} MP`);
    if (l.maxPages) parts.push(`${l.maxPages} pages`);
    return parts.join(', ');
  };

  for (const [key, entry] of Object.entries(manifest.limits)) {
    lines.push(`| \`${key}\` | ${formatLimit(entry.web)} | ${formatLimit(entry.desktop)} |`);
  }
  lines.push(`| **Memory budget** | ${manifest.memoryBudgetMB.web} MB | ${manifest.memoryBudgetMB.desktop} MB |`);
  lines.push('');

  return lines.join('\n');
}

function main() {
  const checkOnly = process.argv.includes('--check');

  const manifest = JSON.parse(fs.readFileSync(MANIFEST_PATH, 'utf8'));
  const schema = JSON.parse(fs.readFileSync(SCHEMA_PATH, 'utf8'));

  validateManifest(manifest, schema);

  const outputs = [
    { filePath: OUT_TS, content: generateTypeScript(manifest) },
    { filePath: OUT_RS, content: generateRust(manifest) },
    { filePath: OUT_MD, content: generatePlatformMatrixMarkdown(manifest) },
  ];

  const drifted = [];
  for (const { filePath, content } of outputs) {
    const rel = path.relative(ROOT, filePath).replaceAll('\\', '/');
    if (checkOnly) {
      const existing = fs.existsSync(filePath) ? fs.readFileSync(filePath, 'utf8') : null;
      if (existing !== content) {
        drifted.push(rel);
      }
    } else {
      fs.mkdirSync(path.dirname(filePath), { recursive: true });
      fs.writeFileSync(filePath, content, 'utf8');
      console.log(`Generated ${rel}`);
    }
  }

  if (checkOnly) {
    if (drifted.length > 0) {
      console.error('Manifest drift detected in:');
      for (const rel of drifted) console.error(`  - ${rel}`);
      console.error('Run `pnpm manifest:generate` to regenerate.');
      process.exit(1);
    }
    console.log(`Manifest check passed (${manifest.tools.length} tools, ${Object.keys(manifest.formats).length} formats, 0 drift).`);
  }
}

main();
