import type { ThemeMode } from '../../hooks/useTheme.ts';

interface TopbarProps {
  activeJobsCount: number;
  theme: ThemeMode;
  onToggleTheme: () => void;
  onOpenSearch: () => void;
  onOpenShortcuts: () => void;
  onSelectHome: () => void;
}

export function Topbar({
  activeJobsCount,
  theme,
  onToggleTheme,
  onOpenSearch,
  onOpenShortcuts,
  onSelectHome,
}: TopbarProps) {
  return (
    <header className="topbar">
      <div className="topbar-inner">
        <div className="topbar-left">
          <a
            href="#home"
            className="wordmark"
            title="Refract — Local-First File Processing Workstation"
            onClick={(e) => {
              e.preventDefault();
              location.hash = '#home';
              onSelectHome();
            }}
          >
            <span className="wordmark-icon" aria-hidden="true">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                <polygon points="12 2 22 20 2 20" />
                <line x1="12" y1="2" x2="12" y2="20" strokeDasharray="2 2" strokeWidth="1.2" />
              </svg>
            </span>
            <span className="wordmark-text">refract</span>
          </a>

          <div
            className="live-status-pill"
            title="Local WebAssembly sandbox. All processing occurs on-device in Web Workers (2 concurrent jobs max)."
          >
            <span className={`status-dot${activeJobsCount > 0 ? ' is-busy' : ''}`} aria-hidden="true" />
            <span className="status-pill-text">
              {activeJobsCount > 0 ? `WASM: ${activeJobsCount} active` : 'WASM: on-device'}
            </span>
          </div>
        </div>

        <div className="topbar-right">
          <button
            type="button"
            className="topbar-search-btn"
            onClick={onOpenSearch}
            title="Search tools, formats, and presets (⌘K / Ctrl+K)"
            aria-label="Search tools"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <circle cx="11" cy="11" r="8" />
              <line x1="21" y1="21" x2="16.65" y2="16.65" />
            </svg>
            <span className="search-btn-text">Quick search</span>
            <kbd className="search-kbd">⌘K</kbd>
          </button>

          <button
            type="button"
            className="icon-btn-subtle"
            onClick={onOpenShortcuts}
            aria-label="Keyboard shortcuts"
            title="Keyboard shortcuts (?)"
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <circle cx="12" cy="12" r="10" />
              <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
              <line x1="12" y1="17" x2="12.01" y2="17" />
            </svg>
          </button>

          <button
            type="button"
            className="theme-toggle"
            onClick={onToggleTheme}
            aria-label={`Theme: ${theme}. Click to change.`}
            title={`Theme: ${theme}`}
          >
            {theme === 'dark' ? (
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
              </svg>
            ) : theme === 'light' ? (
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <circle cx="12" cy="12" r="5" />
                <line x1="12" y1="1" x2="12" y2="3" />
                <line x1="12" y1="21" x2="12" y2="23" />
                <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
                <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
                <line x1="1" y1="12" x2="3" y2="12" />
                <line x1="21" y1="12" x2="23" y2="12" />
                <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
                <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
              </svg>
            ) : (
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
                <line x1="8" y1="21" x2="16" y2="21" />
                <line x1="12" y1="17" x2="12" y2="21" />
              </svg>
            )}
          </button>
        </div>
      </div>
    </header>
  );
}
