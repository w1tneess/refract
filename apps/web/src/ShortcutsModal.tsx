import { useEffect } from 'react';

export function ShortcutsModal({
  isOpen,
  onClose,
}: {
  isOpen: boolean;
  onClose: () => void;
}) {
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    if (isOpen) window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  return (
    <div className="palette-backdrop" onClick={onClose} role="presentation">
      <div
        className="palette-dialog shortcuts-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Keyboard Shortcuts"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="shortcuts-header">
          <div className="shortcuts-title-group">
            <svg
              width="20"
              height="20"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden="true"
            >
              <rect x="2" y="4" width="20" height="16" rx="2" ry="2" />
              <path d="M6 8h.001" />
              <path d="M10 8h.001" />
              <path d="M14 8h.001" />
              <path d="M18 8h.001" />
              <path d="M8 12h.001" />
              <path d="M12 12h.001" />
              <path d="M16 12h.001" />
              <path d="M7 16h10" />
            </svg>
            <h2>Keyboard Shortcuts</h2>
          </div>
          <button type="button" className="job-dismiss-btn" onClick={onClose} aria-label="Close shortcuts">
            ×
          </button>
        </div>

        <div className="shortcuts-grid">
          <div className="shortcut-row">
            <span className="shortcut-desc">Open Command Palette / Tool Search</span>
            <span className="shortcut-keys">
              <kbd>⌘</kbd> <kbd>K</kbd> / <kbd>Ctrl</kbd> <kbd>K</kbd>
            </span>
          </div>

          <div className="shortcut-row">
            <span className="shortcut-desc">Paste image or screenshot directly from clipboard</span>
            <span className="shortcut-keys">
              <kbd>⌘</kbd> <kbd>V</kbd> / <kbd>Ctrl</kbd> <kbd>V</kbd>
            </span>
          </div>

          <div className="shortcut-row">
            <span className="shortcut-desc">Switch to Images domain</span>
            <span className="shortcut-keys">
              <kbd>1</kbd>
            </span>
          </div>

          <div className="shortcut-row">
            <span className="shortcut-desc">Switch to PDF domain</span>
            <span className="shortcut-keys">
              <kbd>2</kbd>
            </span>
          </div>

          <div className="shortcut-row">
            <span className="shortcut-desc">Switch to Audio domain</span>
            <span className="shortcut-keys">
              <kbd>3</kbd>
            </span>
          </div>

          <div className="shortcut-row">
            <span className="shortcut-desc">Show this shortcuts guide</span>
            <span className="shortcut-keys">
              <kbd>?</kbd>
            </span>
          </div>

          <div className="shortcut-row">
            <span className="shortcut-desc">Close any modal or menu</span>
            <span className="shortcut-keys">
              <kbd>Esc</kbd>
            </span>
          </div>
        </div>

        <div className="palette-footer">
          <span>Refract runs 100% locally in your browser via WebAssembly</span>
          <button type="button" className="button subtle-btn" onClick={onClose}>
            Got it
          </button>
        </div>
      </div>
    </div>
  );
}
