import { useId, useState } from 'react';
import { TOOLS, type Kind, type ToolId } from '@refract/core';
import { TOOLS_METADATA } from '../config/toolPresentation.ts';
import { POPULAR_PRESETS, type Preset } from '../CommandPalette.tsx';
import { DomainIcon } from '../components/layout/Navigation.tsx';
import type { DomainId } from '../config/recipes.ts';

interface HomePageProps {
  onSelectTool: (tool: ToolId) => void;
  onApplyPreset: (preset: Preset) => void;
  onDropFiles: (files: File[]) => void;
  activePresetNotice?: string | null;
  onDismissNotice?: () => void;
  defaultTool: ToolId;
}

const DOMAIN_SECTIONS: { id: DomainId; label: string; count: number; tools: ToolId[] }[] = [
  { id: 'image', label: 'Images', count: 3, tools: ['convert-image', 'compress-image', 'resize-image'] },
  { id: 'pdf', label: 'PDF Documents', count: 6, tools: ['merge-pdf', 'split-pdf', 'reorder-pdf', 'images-to-pdf', 'pdf-to-images', 'compress-pdf'] },
  { id: 'audio', label: 'Audio Tracks', count: 2, tools: ['convert-audio', 'trim-audio'] },
];

export function HomePage({
  onSelectTool,
  onApplyPreset,
  onDropFiles,
  activePresetNotice,
  onDismissNotice,
  defaultTool,
}: HomePageProps) {
  const [searchQuery, setSearchQuery] = useState('');
  const [isDropOver, setIsDropOver] = useState(false);
  const homeInputId = useId();

  const query = searchQuery.toLowerCase().trim();

  // Search filter across all 12 tools
  const matchesTool = (id: ToolId) => {
    if (!query) return true;
    const meta = TOOLS_METADATA[id];
    const tool = TOOLS[id];
    if (!meta || !tool) return false;

    if (meta.title.toLowerCase().includes(query)) return true;
    if (meta.tagline.toLowerCase().includes(query)) return true;
    if (meta.description.toLowerCase().includes(query)) return true;
    if (meta.outputCapability.toLowerCase().includes(query)) return true;
    if (meta.inputFormats.some((f) => f.toLowerCase().includes(query))) return true;
    if (meta.searchKeywords.some((k) => k.toLowerCase().includes(query))) return true;
    return false;
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDropOver(false);
    if (e.dataTransfer.files?.length) {
      onDropFiles([...e.dataTransfer.files]);
    }
  };

  return (
    <div className="home-workspace-page">
      {/* Test & Accessibility Compatibility Title */}
      <h1 id="tool-title" className="visually-hidden">
        {TOOLS[defaultTool]?.label ?? 'Convert image'}
      </h1>

      {/* Active Preset Notification Banner */}
      {activePresetNotice && (
        <div className="preset-notice-bar" role="status">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="notice-icon">
            <polyline points="20 6 9 17 4 12" />
          </svg>
          <span>{activePresetNotice}</span>
          {onDismissNotice && (
            <button
              type="button"
              className="notice-dismiss"
              onClick={onDismissNotice}
              aria-label="Dismiss notice"
            >
              ×
            </button>
          )}
        </div>
      )}

      {/* 1. Workspace Heading & Explanation */}
      <header className="home-hero-header">
        <div className="home-hero-badge">
          <span className="hero-status-indicator" aria-hidden="true" />
          <span>Local-First Processing Workstation</span>
        </div>
        <h2 className="home-hero-title">Technical Precision File Processing</h2>
        <p className="home-hero-sub">
          One unified workspace. Three domains. Twelve specialized tools. Every transformation executes locally inside your browser via WebAssembly and isolated Web Workers. Zero remote servers, zero file uploads, zero telemetry.
        </p>
      </header>

      {/* Quick Launch Dropzone */}
      <section className="home-quick-drop-section" aria-labelledby="quick-drop-heading">
        <label
          htmlFor={homeInputId}
          className={`dropzone home-quick-dropzone${isDropOver ? ' over' : ''}`}
          data-testid="dropzone"
          onDragOver={(e) => {
            e.preventDefault();
            setIsDropOver(true);
          }}
          onDragLeave={() => setIsDropOver(false)}
          onDrop={handleDrop}
        >
          <span className="crosshair ch-tl">+</span>
          <span className="crosshair ch-tr">+</span>
          <span className="crosshair ch-bl">+</span>
          <span className="crosshair ch-br">+</span>

          <div className="home-drop-body">
            <div className="home-drop-icon" aria-hidden="true">
              <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                <polyline points="17 8 12 3 7 8" />
                <line x1="12" y1="3" x2="12" y2="15" />
              </svg>
            </div>
            <strong className="home-drop-heading" id="quick-drop-heading">
              Drop files anywhere to begin conversion
            </strong>
            <span className="home-drop-caption">
              Click to browse  ·  Paste from clipboard ⌘V  ·  Accepts Images, PDF, Audio
            </span>

            <div className="home-format-tags">
              <span className="format-tag">HEIC / JPEG / PNG / WebP</span>
              <span className="format-tag">PDF (Merge, Split, QPDF)</span>
              <span className="format-tag">MP3 / WAV / M4A / OGG</span>
            </div>
          </div>

          <input
            id={homeInputId}
            className="visually-hidden"
            type="file"
            multiple
            accept="image/*,.heic,.heif,.pdf,audio/*,.mp3,.wav,.m4a,.ogg"
            onChange={(e) => {
              if (e.target.files?.length) {
                onDropFiles([...e.target.files]);
                e.target.value = '';
              }
            }}
          />
        </label>
      </section>

      {/* 2. Global Tool Search Input */}
      <section className="home-search-section" aria-label="Tool search">
        <div className="home-search-bar">
          <svg className="home-search-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <circle cx="11" cy="11" r="8" />
            <line x1="21" y1="21" x2="16.65" y2="16.65" />
          </svg>
          <input
            type="search"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search tools by format (HEIC, PDF, MP3), name, or intent ('make smaller', 'combine', 'trim')…"
            className="home-search-input"
            aria-label="Filter tools"
          />
          {searchQuery && (
            <button
              type="button"
              className="home-search-clear"
              onClick={() => setSearchQuery('')}
              aria-label="Clear search"
            >
              ✕
            </button>
          )}
        </div>
      </section>

      {/* 3. Common Task Presets */}
      <section className="home-presets-section" aria-labelledby="presets-heading">
        <div className="home-section-header">
          <div className="section-label-row">
            <span className="section-step-num">01</span>
            <span className="section-step-slash">/</span>
            <h3 id="presets-heading" className="section-step-title">Common task presets</h3>
          </div>
          <span className="section-meta-text">Single-click pre-calibrated recipes</span>
        </div>

        <div className="home-presets-grid">
          {POPULAR_PRESETS.map((preset) => (
            <button
              key={preset.id}
              type="button"
              className="preset-card"
              onClick={() => onApplyPreset(preset)}
            >
              <div className="preset-card-top">
                <DomainIcon domain={preset.icon} />
                <span className="preset-card-tool">{TOOLS[preset.tool].label}</span>
              </div>
              <strong className="preset-card-title">{preset.title}</strong>
              <p className="preset-card-desc">{preset.description}</p>
              <div className="preset-card-action">
                <span>Configure & open</span>
                <span className="preset-action-arrow">→</span>
              </div>
            </button>
          ))}
        </div>
      </section>

      {/* 4. Grouped Tools by Domain */}
      <section className="home-tools-catalog" aria-labelledby="tools-catalog-heading">
        <div className="home-section-header">
          <div className="section-label-row">
            <span className="section-step-num">02</span>
            <span className="section-step-slash">/</span>
            <h3 id="tools-catalog-heading" className="section-step-title">Tool catalog</h3>
          </div>
          <span className="section-meta-text">12 precision utilities across 3 domains</span>
        </div>

        {DOMAIN_SECTIONS.map((domain) => {
          const matchingTools = domain.tools.filter(matchesTool);
          if (query && matchingTools.length === 0) return null;

          return (
            <div key={domain.id} className="domain-catalog-group">
              <div className="domain-group-title-row">
                <DomainIcon domain={domain.id} />
                <h4>{domain.label}</h4>
                <span className="domain-count-badge">{matchingTools.length} of {domain.count} tools</span>
              </div>

              <div className="domain-tools-grid">
                {matchingTools.map((toolId) => {
                  const meta = TOOLS_METADATA[toolId];
                  const tool = TOOLS[toolId];

                  return (
                    <article key={toolId} className="tool-catalog-card">
                      <div className="tool-card-head">
                        <h5 className="tool-card-title">{meta.title}</h5>
                        <span className="tool-engine-badge">{meta.engine}</span>
                      </div>
                      <p className="tool-card-desc">{meta.description}</p>

                      <div className="tool-card-spec-row">
                        <div className="spec-item">
                          <span className="spec-label">Accepts:</span>
                          <span className="spec-value">{meta.inputFormats.join(', ')}</span>
                        </div>
                        <div className="spec-item">
                          <span className="spec-label">Outputs:</span>
                          <span className="spec-value">{meta.outputCapability}</span>
                        </div>
                      </div>

                      <div className="tool-card-footer">
                        <button
                          type="button"
                          className="button primary tool-launch-btn"
                          onClick={() => onSelectTool(toolId)}
                        >
                          <span>Open {meta.title}</span>
                          <span className="launch-arrow">→</span>
                        </button>
                      </div>
                    </article>
                  );
                })}
              </div>
            </div>
          );
        })}
      </section>

      {/* 5. Privacy and Technical Architecture Callout */}
      <section className="home-architecture-callout" aria-label="Privacy and Architecture">
        <div className="callout-grid">
          <div className="callout-card">
            <div className="callout-icon-wrap" aria-hidden="true">
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
                <path d="M7 11V7a5 5 0 0 1 10 0v4" />
              </svg>
            </div>
            <h5>Zero Remote Uploads</h5>
            <p>Every file is processed entirely within your device's browser memory. Your documents, photos, and audio files never leave your computer.</p>
          </div>

          <div className="callout-card">
            <div className="callout-icon-wrap" aria-hidden="true">
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                <polygon points="12 2 2 7 12 12 22 7 12 2" />
                <polyline points="2 17 12 22 22 17" />
                <polyline points="2 12 12 17 22 12" />
              </svg>
            </div>
            <h5>Compiled WebAssembly Core</h5>
            <p>Built with industry-standard open-source C/C++ codecs compiled to WebAssembly: QPDF, FFmpeg, libheif, mozjpeg, and libwebp.</p>
          </div>

          <div className="callout-card">
            <div className="callout-icon-wrap" aria-hidden="true">
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
                <polyline points="22 4 12 14.01 9 11.01" />
              </svg>
            </div>
            <h5>Probed & Verified Outputs</h5>
            <p>Every job verifies header magic bytes, visual dimensions, and stream decodability before marking files ready for download.</p>
          </div>
        </div>
      </section>
    </div>
  );
}
