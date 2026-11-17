import type { Kind, ToolId } from '@refract/core';

export type DomainId = 'image' | 'pdf' | 'audio';

export interface DomainDefinition {
  id: DomainId;
  /** Plural label used in navigation and headings. */
  label: string;
  /** Short label for tight spaces such as the mobile bar. */
  shortLabel: string;
  description: string;
  tools: ToolId[];
}

export const DOMAINS: DomainDefinition[] = [
  {
    id: 'image',
    label: 'Images',
    shortLabel: 'Images',
    description: 'Convert, compress and resize photos.',
    tools: ['convert-image', 'compress-image', 'resize-image'],
  },
  {
    id: 'pdf',
    label: 'PDF documents',
    shortLabel: 'PDF',
    description: 'Merge, split, reorder, compress and convert PDFs.',
    tools: ['merge-pdf', 'split-pdf', 'reorder-pdf', 'images-to-pdf', 'pdf-to-images', 'compress-pdf'],
  },
  {
    id: 'audio',
    label: 'Audio',
    shortLabel: 'Audio',
    description: 'Convert and trim sound files.',
    tools: ['convert-audio', 'trim-audio'],
  },
];

export const ALL_TOOLS: ToolId[] = DOMAINS.flatMap((domain) => domain.tools);

export const domainOfTool = (tool: ToolId): DomainDefinition => DOMAINS.find((domain) => domain.tools.includes(tool)) ?? DOMAINS[0]!;

export const domainIdOfTool = (tool: ToolId): DomainId => domainOfTool(tool).id;

export const domainById = (id: DomainId): DomainDefinition => DOMAINS.find((domain) => domain.id === id) ?? DOMAINS[0]!;

/** The tool a file kind lands on when it is dropped without choosing a tool first. */
export function defaultToolForKind(kind: Kind, count = 1): ToolId {
  if (kind === 'audio') return 'convert-audio';
  if (kind === 'pdf') return count > 1 ? 'merge-pdf' : 'compress-pdf';
  return 'convert-image';
}

const IMAGE_NAME = /\.(heic|heif|jpe?g|png|webp|avif|gif|bmp|tiff?)$/i;
const AUDIO_NAME = /\.(mp3|wav|m4a|ogg|oga|aac|flac|opus)$/i;

/** Detect what a dropped file is, using the MIME type first and the file name second. */
export function kindOfFile(file: { name: string; type?: string }): Kind | undefined {
  const mime = file.type ?? '';
  if (mime.startsWith('image/')) return 'image';
  if (mime === 'application/pdf') return 'pdf';
  if (mime.startsWith('audio/')) return 'audio';
  if (/\.pdf$/i.test(file.name)) return 'pdf';
  if (IMAGE_NAME.test(file.name)) return 'image';
  if (AUDIO_NAME.test(file.name)) return 'audio';
  return undefined;
}
