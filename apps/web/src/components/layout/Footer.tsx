import type { ToolId } from '@refract/core';

interface FooterProps {
  onSelectTool: (tool: ToolId) => void;
  onOpenShortcuts: () => void;
  onOpenSearch: () => void;
}

export function Footer({ onSelectTool, onOpenShortcuts, onOpenSearch }: FooterProps) {
  return (
    <footer className="footer" aria-label="Page footer">
      <div className="footer-inner">
        <div className="footer-left">
          <div className="footer-brand">
            <span className="wordmark-icon" aria-hidden="true">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                <polygon points="12 2 22 20 2 20" />
                <line x1="12" y1="2" x2="12" y2="20" strokeDasharray="2 2" strokeWidth="1.2" />
              </svg>
            </span>
            <strong className="footer-name">refract</strong>
          </div>
          <p className="footer-tagline">
            Precision, local-first file conversion workstation. Powered by WebAssembly and Web Workers. Zero remote uploads, no telemetry, no accounts.
          </p>
        </div>

        <div className="footer-right">
          <div className="footer-nav-col">
            <h4>Tools</h4>
            <a href="#convert-image" onClick={() => onSelectTool('convert-image')}>Images (3)</a>
            <a href="#merge-pdf" onClick={() => onSelectTool('merge-pdf')}>PDF (6)</a>
            <a href="#convert-audio" onClick={() => onSelectTool('convert-audio')}>Audio (2)</a>
          </div>

          <div className="footer-nav-col">
            <h4>Workstation</h4>
            <button type="button" className="footer-link-btn" onClick={onOpenShortcuts}>
              Shortcuts (?)
            </button>
            <button type="button" className="footer-link-btn" onClick={onOpenSearch}>
              Command Palette (⌘K)
            </button>
            <a href="https://github.com/w1tneess/refract" target="_blank" rel="noopener noreferrer">
              GitHub Repository
            </a>
          </div>
        </div>
      </div>

      <div className="footer-bottom">
        <p>© 2026 Refract Project. Released under GPL-3.0-or-later. 100% on-device processing.</p>
      </div>
    </footer>
  );
}
