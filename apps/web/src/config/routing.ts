import { TOOLS, type Kind, type ToolId } from '@refract/core';
import { defaultToolForKind, kindOfFile } from './domains.ts';

export interface FileGroup {
  tool: ToolId;
  kind: Kind;
  files: File[];
}

/**
 * Sort dropped files into the tools that can handle them.
 *
 * If the current tool already accepts a kind, it keeps that kind so the
 * settings the person just tuned are used. Everything else goes to the
 * obvious default tool, which means dropping a mixed batch on the home
 * screen still does something useful. Unknown files go to the current tool
 * so the job card can explain what is wrong with them.
 */
export function groupFilesByTool(files: File[], currentTool: ToolId): FileGroup[] {
  const kinds = new Map<Kind, File[]>();
  const unknown: File[] = [];

  for (const file of files) {
    const kind = kindOfFile(file);
    if (!kind) {
      unknown.push(file);
      continue;
    }
    const existing = kinds.get(kind) ?? [];
    existing.push(file);
    kinds.set(kind, existing);
  }

  const groups: FileGroup[] = [];
  for (const [kind, group] of kinds) {
    const tool = TOOLS[currentTool].accepts === kind ? currentTool : defaultToolForKind(kind, group.length);
    groups.push({ tool, kind, files: group });
  }

  if (unknown.length) {
    const kind = TOOLS[currentTool].accepts;
    const existing = groups.find((group) => group.tool === currentTool);
    if (existing) existing.files.push(...unknown);
    else groups.push({ tool: currentTool, kind, files: unknown });
  }

  // Keep the order files were dropped in.
  return groups.sort((a, b) => files.indexOf(a.files[0]!) - files.indexOf(b.files[0]!));
}

const KIND_LABEL: Record<Kind, [string, string]> = {
  image: ['image', 'images'],
  pdf: ['PDF', 'PDFs'],
  audio: ['audio file', 'audio files'],
};

export function describeGroups(groups: FileGroup[]): string {
  return groups
    .map((group) => {
      const [one, many] = KIND_LABEL[group.kind];
      const noun = group.files.length === 1 ? one : many;
      return `${group.files.length} ${noun} → ${TOOLS[group.tool].label}`;
    })
    .join(' · ');
}
