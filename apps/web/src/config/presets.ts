import type { Options, ToolId } from '@refract/core';
import type { DomainId } from './domains.ts';

export interface Preset {
  id: string;
  title: string;
  description: string;
  tool: ToolId;
  domain: DomainId;
  options?: Options;
  /** Shown on the home screen and at the top of the command palette. */
  popular?: boolean;
}

/**
 * One shared list of one-click settings.
 *
 * Keep the option keys in sync with the specs in `@refract/core`; a preset that
 * writes an unknown key silently does nothing.
 */
export const PRESETS: Preset[] = [
  {
    id: 'heic-to-jpeg',
    title: 'HEIC to JPEG',
    description: 'Standard JPEG at 90% quality for sharing anywhere.',
    tool: 'convert-image',
    domain: 'image',
    options: { to: 'jpeg', quality: 90 },
    popular: true,
  },
  {
    id: 'image-to-webp',
    title: 'Image to WebP',
    description: 'Smaller web-ready photos at 85% quality.',
    tool: 'convert-image',
    domain: 'image',
    options: { to: 'webp', quality: 85 },
  },
  {
    id: 'compress-under-500k',
    title: 'Compress under 500 KB',
    description: 'Automatic quality search to hit a file-size target.',
    tool: 'compress-image',
    domain: 'image',
    options: { to: 'jpeg', targetKB: 500 },
    popular: true,
  },
  {
    id: 'resize-1920',
    title: 'Resize to 1920 px wide',
    description: 'Full-HD width, height follows the aspect ratio.',
    tool: 'resize-image',
    domain: 'image',
    options: { unit: 'px', width: 1920, height: 0 },
  },
  {
    id: 'merge-pdfs',
    title: 'Merge PDFs',
    description: 'Join several documents into one, in the order added.',
    tool: 'merge-pdf',
    domain: 'pdf',
    popular: true,
  },
  {
    id: 'compress-pdf',
    title: 'Shrink a PDF',
    description: 'Lossless stream compression with QPDF.',
    tool: 'compress-pdf',
    domain: 'pdf',
    popular: true,
  },
  {
    id: 'pdf-pages-to-png',
    title: 'PDF pages to PNG',
    description: 'One image per page, bundled in a ZIP.',
    tool: 'pdf-to-images',
    domain: 'pdf',
  },
  {
    id: 'photos-to-pdf',
    title: 'Photos to PDF',
    description: 'Compile images into a single multi-page document.',
    tool: 'images-to-pdf',
    domain: 'pdf',
  },
  {
    id: 'to-mp3',
    title: 'Convert to MP3',
    description: '192 kbps, a good default for most listening.',
    tool: 'convert-audio',
    domain: 'audio',
    options: { to: 'mp3', bitrate: '192k' },
    popular: true,
  },
  {
    id: 'trim-first-30s',
    title: 'Trim the first 30 s',
    description: 'Quick ringtone or short clip from the start.',
    tool: 'trim-audio',
    domain: 'audio',
    options: { start: 0, end: 30 },
    popular: true,
  },
];

export const POPULAR_PRESETS = PRESETS.filter((preset) => preset.popular);

export const presetsForDomain = (domain: DomainId) => PRESETS.filter((preset) => preset.domain === domain);

export const presetsForTool = (tool: ToolId) => PRESETS.filter((preset) => preset.tool === tool);
