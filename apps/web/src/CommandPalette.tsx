import { useEffect, useRef, useState } from 'react';
import { TOOLS, type Options, type ToolId } from '@refract/core';

export type Preset = {
  id: string;
  title: string;
  description: string;
  tool: ToolId;
  options?: Options;
  icon: 'image' | 'pdf' | 'audio';
};

export const POPULAR_PRESETS: Preset[] = [
  {
    id: 'heic-to-jpg',
    title: 'iPhone HEIC to JPEG',
    description: 'Convert Apple Live Photos and camera HEIC files to standard JPEG',
    tool: 'convert-image',
    options: { to: 'jpeg', quality: 90 },
    icon: 'image',
  },
  {
    id: 'next-gen-webp',
    title: 'WebP optimization',
    description: 'Convert photos to WebP format for smaller file sizes',
    tool: 'convert-image',
    options: { to: 'webp', quality: 85 },
    icon: 'image',
  },
  {
    id: 'compress-discord',
    title: 'Compress under 25 MB',
    description: 'Binary quality search to compress photos for chat or email attachments',
    tool: 'compress-image',
    options: { targetKB: 24000 },
    icon: 'image',
  },
  {
    id: 'merge-docs',
    title: 'Merge PDFs',
    description: 'Combine multiple PDF scans and documents into one file',
    tool: 'merge-pdf',
    icon: 'pdf',
  },
  {
    id: 'shrink-pdf',
    title: 'Compress PDF',
    description: 'Stream recompression and object linearization powered by QPDF',
    tool: 'compress-pdf',
    icon: 'pdf',
  },
  {
    id: 'trim-audio-ringtone',
    title: 'Trim audio',
    description: 'Cut audio track with millisecond boundaries for clips and ringtones',
    tool: 'trim-audio',
    options: { start: '00:00', end: '00:30' },
    icon: 'audio',
  },
];

type Item =
  | { type: 'tool'; id: ToolId; label: string; blurb: string; accepts: string }
  | { type: 'preset'; preset: Preset };

const kindLabel: Record<string, string> = {
  image: 'Image',
  pdf: 'PDF',
  audio: 'Audio',
};

export function CommandPalette({
  isOpen,
  onClose,
  onSelectTool,
  onApplyPreset,
}: {
  isOpen: boolean;
  onClose: () => void;
  onSelectTool: (tool: ToolId) => void;
  onApplyPreset: (preset: Preset) => void;
}) {
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const allItems: Item[] = [
    ...POPULAR_PRESETS.map((p): Item => ({ type: 'preset', preset: p })),
    ...(Object.entries(TOOLS) as [ToolId, (typeof TOOLS)[ToolId]][]).map(
      ([id, t]): Item => ({
        type: 'tool',
        id,
        label: t.label,
        blurb: t.blurb,
        accepts: t.accepts,
      }),
    ),
  ];

  const q = query.toLowerCase().trim();
  const filtered = allItems.filter((item) => {
    if (!q) return true;
    if (item.type === 'tool') {
      return (
        item.label.toLowerCase().includes(q) ||
        item.blurb.toLowerCase().includes(q) ||
        item.id.toLowerCase().includes(q) ||
        item.accepts.toLowerCase().includes(q)
      );
    }
    return (
      item.preset.title.toLowerCase().includes(q) ||
      item.preset.description.toLowerCase().includes(q) ||
      item.preset.tool.toLowerCase().includes(q)
    );
  });

  useEffect(() => {
    if (isOpen) {
      setQuery('');
      setActiveIndex(0);
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  }, [isOpen]);

  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  const selectItem = (item: Item) => {
    if (item.type === 'tool') {
      onSelectTool(item.id);
    } else {
      onApplyPreset(item.preset);
    }
    onClose();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActiveIndex((prev) => (filtered.length ? (prev + 1) % filtered.length : 0));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActiveIndex((prev) => (filtered.length ? (prev - 1 + filtered.length) % filtered.length : 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (filtered[activeIndex]) {
        selectItem(filtered[activeIndex]);
      }
    }
  };

  if (!isOpen) return null;

  return (
    <div className="palette-backdrop" onClick={onClose} role="presentation">
      <div
        className="palette-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Command Palette"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        <div className="palette-search-wrap">
          <svg
            className="palette-search-icon"
            width="18"
            height="18"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <circle cx="11" cy="11" r="8" />
            <line x1="21" y1="21" x2="16.65" y2="16.65" />
          </svg>
          <input
            ref={inputRef}
            type="text"
            className="palette-input"
            placeholder="Search tools, formats, or presets..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <kbd className="palette-kbd">Esc</kbd>
        </div>

        <div className="palette-results" role="listbox">
          {filtered.length === 0 ? (
            <div className="palette-empty">No tools or presets match &ldquo;{query}&rdquo;</div>
          ) : (
            filtered.map((item, idx) => {
              const isSelected = idx === activeIndex;
              if (item.type === 'preset') {
                return (
                  <div
                    key={`preset-${item.preset.id}`}
                    role="option"
                    aria-selected={isSelected}
                    className={`palette-item preset-item${isSelected ? ' is-selected' : ''}`}
                    onClick={() => selectItem(item)}
                    onMouseEnter={() => setActiveIndex(idx)}
                  >
                    <span className="palette-badge preset-badge">Preset</span>
                    <div className="palette-info">
                      <strong className="palette-title">{item.preset.title}</strong>
                      <span className="palette-desc">{item.preset.description}</span>
                    </div>
                    <span className="palette-action">Use preset</span>
                  </div>
                );
              }

              return (
                <div
                  key={`tool-${item.id}`}
                  role="option"
                  aria-selected={isSelected}
                  className={`palette-item tool-item${isSelected ? ' is-selected' : ''}`}
                  onClick={() => selectItem(item)}
                  onMouseEnter={() => setActiveIndex(idx)}
                >
                  <span className={`palette-badge kind-${item.accepts}`}>{kindLabel[item.accepts] ?? item.accepts}</span>
                  <div className="palette-info">
                    <strong className="palette-title">{item.label}</strong>
                    <span className="palette-desc">{item.blurb}</span>
                  </div>
                  <span className="palette-action">Open tool</span>
                </div>
              );
            })
          )}
        </div>

        <div className="palette-footer">
          <span>Navigate with <kbd>↑</kbd> <kbd>↓</kbd></span>
          <span>Select with <kbd>Enter</kbd></span>
          <span>Close with <kbd>Esc</kbd></span>
        </div>
      </div>
    </div>
  );
}
