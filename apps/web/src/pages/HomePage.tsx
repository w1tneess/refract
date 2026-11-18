import { useId, useState } from 'react';
import { TOOLS, type ToolId } from '@refract/core';
import { ALL_TOOLS, DOMAINS } from '../config/domains.ts';
import { POPULAR_PRESETS, type Preset } from '../config/presets.ts';
import { TOOLS_METADATA } from '../config/toolPresentation.ts';
import { DomainIcon } from '../components/layout/DomainIcon.tsx';

interface HomePageProps {
  jobCount: number;
  onOpenQueue: () => void;
  onSelectTool: (tool: ToolId) => void;
  onApplyPreset: (preset: Preset) => void;
  onDropFiles: (files: File[]) => void;
  activePresetNotice?: string | null;
  onDismissNotice?: () => void;
}

function matchesTool(id: ToolId, query: string): boolean {
  if (!query) return true;
  const meta = TOOLS_METADATA[id];
  const label = TOOLS[id].label.toLowerCase();
  if (label.includes(query)) return true;
  if (!meta) return false;
  return (
    meta.tagline.toLowerCase().includes(query) ||
    meta.description.toLowerCase().includes(query) ||
    meta.outputCapability.toLowerCase().includes(query) ||
    meta.inputFormats.some((f) => f.toLowerCase().includes(query)) ||
    meta.searchKeywords.some((k) => k.toLowerCase().includes(query))
  );
}

export function HomePage({
  jobCount,
  onOpenQueue,
  onSelectTool,
  onApplyPreset,
  onDropFiles,
  activePresetNotice,
  onDismissNotice,
}: HomePageProps) {
  const [searchQuery, setSearchQuery] = useState('');
  const [isDropOver, setIsDropOver] = useState(false);
  const inputId = useId();
  const query = searchQuery.toLowerCase().trim();

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDropOver(false);
    if (e.dataTransfer.files?.length) onDropFiles([...e.dataTransfer.files]);
  };

  const matchingDomains = DOMAINS.map((domain) => ({
    ...domain,
    tools: domain.tools.filter((id) => matchesTool(id, query)),
  }));
  const hasMatches = matchingDomains.some((domain) => domain.tools.length > 0);

  return (
    <div className="home-page">
      {activePresetNotice && (
        <div className="notice-bar" role="status">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <polyline points="20 6 9 17 4 12" />
          </svg>
          <span>{activePresetNotice}</span>
          {onDismissNotice && (
            <button type="button" className="notice-dismiss" onClick={onDismissNotice} aria-label="Dismiss notice">
              ×
            </button>
          )}
        </div>
      )}

      {jobCount > 0 && (
        <div className="queue-strip">
          <span className="queue-strip-dot" aria-hidden="true" />
          <span>{jobCount === 1 ? '1 file in the queue.' : `${jobCount} files in the queue.`}</span>
          <button type="button" className="link queue-strip-link" onClick={onOpenQueue}>
            Return to queue
          </button>
        </div>
      )}

      <header className="home-hero">
        <span className="home-eyebrow">Images · PDF · Audio</span>
        <h1 className="home-title">Drop a file, get it done</h1>
        <p className="home-sub">
          Refract picks the right tool for what you drop, then converts it in your browser. No uploads, no accounts, no
          waiting on a server.
        </p>
      </header>

      <label
        htmlFor={inputId}
        className={`dropzone home-dropzone${isDropOver ? ' over' : ''}`}
        data-testid="dropzone"
        onDragOver={(e) => {
          e.preventDefault();
          e.stopPropagation();
          setIsDropOver(true);
        }}
        onDragLeave={() => setIsDropOver(false)}
        onDrop={handleDrop}
      >
        <span className="dropzone-inner">
          <span className="dropzone-icon" aria-hidden="true">
            <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
              <polyline points="17 8 12 3 7 8" />
              <line x1="12" y1="3" x2="12" y2="15" />
            </svg>
          </span>
          <span className="dropzone-title">Drop files here, or browse</span>
          <span className="dropzone-sub">
            Images, PDF documents, and audio · paste a screenshot with <kbd>Ctrl&nbsp;V</kbd>
          </span>
          <span className="dropzone-formats">HEIC · JPG · PNG · WebP · PDF · MP3 · WAV · M4A · OGG</span>
        </span>
        <input
          id={inputId}
          className="visually-hidden"
          type="file"
          multiple
          accept="image/*,.heic,.heif,.pdf,audio/*,.mp3,.wav,.m4a,.ogg,.oga,.aac,.flac,.opus"
          onChange={(e) => {
            if (e.target.files?.length) {
              onDropFiles([...e.target.files]);
              e.target.value = '';
            }
          }}
        />
      </label>

      <section className="home-section" aria-labelledby="presets-heading">
        <div className="home-section-head">
          <h2 id="presets-heading" className="home-section-title">
            Common tasks
          </h2>
          <span className="home-section-note">Open a tool with sensible settings</span>
        </div>
        <div className="preset-grid">
          {POPULAR_PRESETS.map((preset) => (
            <button key={preset.id} type="button" className="preset-chip" onClick={() => onApplyPreset(preset)}>
              <span className="preset-chip-icon" aria-hidden="true">
                <DomainIcon domain={preset.domain} size={16} />
              </span>
              <span className="preset-chip-body">
                <strong>{preset.title}</strong>
                <small>{preset.description}</small>
              </span>
            </button>
          ))}
        </div>
      </section>

      <section className="home-section" aria-labelledby="tools-heading">
        <div className="home-section-head">
          <h2 id="tools-heading" className="home-section-title">
            All tools
          </h2>
          <span className="home-section-note">
            {ALL_TOOLS.length} tools across {DOMAINS.length} categories
          </span>
        </div>

        <div className="home-search">
          <svg className="home-search-icon" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <circle cx="11" cy="11" r="7.5" />
            <line x1="20.5" y1="20.5" x2="16.2" y2="16.2" />
          </svg>
          <input
            type="search"
            name="tool-search"
            autoComplete="off"
            spellCheck={false}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search tools, e.g. HEIC, merge, MP3…"
            className="home-search-input"
            aria-label="Filter tools"
          />
          {searchQuery && (
            <button type="button" className="home-search-clear" onClick={() => setSearchQuery('')} aria-label="Clear search">
              ×
            </button>
          )}
        </div>

        {matchingDomains.map((domain) => {
          if (!domain.tools.length) return null;
          return (
            <div className="domain-block" key={domain.id}>
              <div className="domain-head">
                <span className="domain-head-icon" aria-hidden="true">
                  <DomainIcon domain={domain.id} size={15} />
                </span>
                <h3>{domain.label}</h3>
                <span className="domain-count">
                  {domain.tools.length} {domain.tools.length === 1 ? 'tool' : 'tools'}
                </span>
                <span className="domain-desc">{domain.description}</span>
              </div>
              <div className="tool-grid">
                {domain.tools.map((id) => {
                  const meta = TOOLS_METADATA[id];
                  return (
                    <button key={id} type="button" className="tool-tile" onClick={() => onSelectTool(id)}>
                      <strong>{TOOLS[id].label}</strong>
                      <span className="tool-tile-desc">{meta?.tagline ?? TOOLS[id].blurb}</span>
                      <span className="tool-tile-meta">{meta?.inputFormats.join(' · ')}</span>
                    </button>
                  );
                })}
              </div>
            </div>
          );
        })}

        {!hasMatches && (
          <div className="home-empty" role="status">
            <p>No tools match “{searchQuery}”. Try HEIC, PDF, merge, compress, or trim.</p>
            <button type="button" className="button" onClick={() => setSearchQuery('')}>
              Clear search
            </button>
          </div>
        )}
      </section>
    </div>
  );
}
