import type { Options, ToolId } from '@refract/core';

export type DomainId = 'image' | 'pdf' | 'audio';

export interface Recipe {
  id: string;
  label: string;
  description: string;
  tool: ToolId;
  options?: Options;
}

export const RECIPES_BY_DOMAIN: Record<DomainId, Recipe[]> = {
  image: [
    {
      id: 'heic-to-jpg',
      label: 'HEIC to JPEG',
      description: 'Standard 85% quality JPEG for web and cross-platform sharing',
      tool: 'convert-image',
      options: { format: 'jpeg', quality: 85 },
    },
    {
      id: 'png-to-webp',
      label: 'PNG to WebP (Lossless)',
      description: 'Convert lossless PNGs to efficient WebP for web delivery',
      tool: 'convert-image',
      options: { format: 'webp', quality: 90 },
    },
    {
      id: 'batch-compress-500k',
      label: 'Compress (<500 KB)',
      description: 'Binary quality search to bring photos under 500 KB',
      tool: 'compress-image',
      options: { maxBytes: 500_000 },
    },
    {
      id: 'resize-hd',
      label: 'Resize to 1920px',
      description: 'Lanczos-3 scale to Full HD width, auto-calculating height',
      tool: 'resize-image',
      options: { width: 1920, height: 0 },
    },
  ],
  pdf: [
    {
      id: 'merge-scans',
      label: 'Merge scanned pages',
      description: 'Combine loose PDF scans into one sequenced document',
      tool: 'merge-pdf',
    },
    {
      id: 'compress-qpdf',
      label: 'Lossless QPDF compression',
      description: 'Re-compress Flate streams and linearize objects',
      tool: 'compress-pdf',
    },
    {
      id: 'pdf-to-png-zip',
      label: 'PDF to PNG pages (ZIP)',
      description: 'Extract every page as high-res PNG inside a ZIP',
      tool: 'pdf-to-images',
    },
    {
      id: 'compile-images-pdf',
      label: 'Images to single PDF',
      description: 'Compile photos or scanned pages into a single document',
      tool: 'images-to-pdf',
    },
  ],
  audio: [
    {
      id: 'convert-mp3-192k',
      label: 'Convert to MP3 (192 kbps)',
      description: 'High-quality MP3 encoding for podcasts and music',
      tool: 'convert-audio',
      options: { format: 'mp3', bitrate: '192k' },
    },
    {
      id: 'extract-wav-pcm',
      label: 'Extract WAV (PCM)',
      description: 'Uncompressed CD-quality audio extraction for editing',
      tool: 'convert-audio',
      options: { format: 'wav' },
    },
    {
      id: 'trim-first-30s',
      label: 'Trim ringtone (0:00 – 0:30)',
      description: 'Extract first 30 seconds of an audio track',
      tool: 'trim-audio',
      options: { start: '00:00', end: '00:30' },
    },
  ],
};
