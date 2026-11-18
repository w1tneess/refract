import { useEffect, useId, useRef, useState } from 'react';
import { TOOLS, type ToolId } from '@refract/core';
import { POPULAR_PRESETS, type Preset } from './config/presets.ts';
import { useFocusTrap } from './hooks/useFocusTrap.ts';

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
  const dialogRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const resultsRef = useRef<HTMLDivElement>(null);
  const baseId = useId();
  const listboxId = `${baseId}-listbox`;

  useFocusTrap(isOpen, dialogRef, inputRef);

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
      item.preset.tool.toLowerCase().includes(q) ||
      item.preset.domain.toLowerCase().includes(q)
    );
  });

  useEffect(() => {
    if (isOpen) {
      setQuery('');
      setActiveIndex(0);
    }
  }, [isOpen]);

  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  useEffect(() => {
    if (!isOpen || !resultsRef.current) return;
    const selected = resultsRef.current.querySelector<HTMLElement>('.palette-item.is-selected');
    selected?.scrollIntoView({ block: 'nearest' });
  }, [activeIndex, isOpen]);

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

  const activeOptionId = filtered[activeIndex] ? `${baseId}-opt-${activeIndex}` : undefined;

  return (
    <div className="palette-backdrop" onClick={onClose} role="presentation">
      <div
        ref={dialogRef}
        className="palette-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        <div className="palette-search-wrap">
          <svg
            className="palette-search-icon"
            width="17"
            height="17"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.6"
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
            name="command-search"
            role="combobox"
            aria-label="Search tools and presets"
            aria-expanded="true"
            aria-autocomplete="list"
            aria-controls={listboxId}
            aria-activedescendant={activeOptionId}
            autoComplete="off"
            spellCheck={false}
            className="palette-input"
            placeholder="Search tools or presets, e.g. HEIC, merge…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <kbd className="palette-kbd">Esc</kbd>
        </div>

        <div ref={resultsRef} id={listboxId} className="palette-results" role="listbox" aria-label="Tools and presets">
          {filtered.length === 0 ? (
            <div className="palette-empty" role="status">
              <p>No tools or presets match “{query}”.</p>
              <button
                type="button"
                className="button"
                onClick={() => {
                  setQuery('');
                  inputRef.current?.focus();
                }}
              >
                Clear search
              </button>
            </div>
          ) : (
            filtered.map((item, idx) => {
              const isSelected = idx === activeIndex;
              const optionId = `${baseId}-opt-${idx}`;
              if (item.type === 'preset') {
                return (
                  <div
                    key={`preset-${item.preset.id}`}
                    id={optionId}
                    role="option"
                    aria-selected={isSelected}
                    className={`palette-item preset-item${isSelected ? ' is-selected' : ''}`}
                    onClick={() => selectItem(item)}
                    onMouseEnter={() => setActiveIndex(idx)}
                  >
                    <span className={`palette-badge kind-${item.preset.domain}`}>{kindLabel[item.preset.domain] ?? 'Preset'}</span>
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
                  id={optionId}
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
          <span>Navigate <kbd>↑</kbd> <kbd>↓</kbd></span>
          <span>Select <kbd>Enter</kbd></span>
          <span>Close <kbd>Esc</kbd></span>
        </div>
      </div>
    </div>
  );
}
