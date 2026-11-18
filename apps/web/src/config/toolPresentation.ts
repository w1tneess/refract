import { MANIFEST_FORMATS, TOOL_IDS, TOOL_MANIFEST, type Kind, type ToolId } from '@refract/core';

export interface ToolPresentation {
  id: ToolId;
  domain: Kind;
  title: string;
  tagline: string;
  description: string;
  inputFormats: string[];
  outputCapability: string;
  engine: string;
  searchKeywords: string[];
  popular?: boolean;
}

export const TOOLS_METADATA: Record<ToolId, ToolPresentation> = Object.fromEntries(
  TOOL_IDS.map((id) => {
    const spec = TOOL_MANIFEST[id];
    const formats = spec.acceptedFormats.web.map((f) => MANIFEST_FORMATS[f].displayName);
    const inputFormats =
      id === 'compress-pdf' && spec.capabilities.passwordUnlock.web
        ? ['PDF (with user/owner password unlock)']
        : formats;
    return [
      id,
      {
        id,
        domain: spec.domain,
        title: spec.label,
        tagline: spec.tagline,
        description: spec.description,
        inputFormats,
        outputCapability: spec.outputCapability,
        engine: spec.platformSupport.web.engine,
        searchKeywords: [...spec.searchKeywords],
        popular: spec.popular,
      } satisfies ToolPresentation,
    ];
  }),
) as Record<ToolId, ToolPresentation>;
