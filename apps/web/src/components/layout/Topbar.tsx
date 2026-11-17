import { TOOLS, type ToolId } from '@refract/core';
import type { ThemeMode } from '../../hooks/useTheme.ts';
import { domainOfTool } from '../../config/domains.ts';

interface TopbarProps {
  tool: ToolId;
  isHome: boolean;
  jobCount: number;
  runningCount: number;
  theme: ThemeMode;
  onToggleTheme: () => void;
  onOpenSearch: () => void;
  onOpenShortcuts: () => void;
  onSelectHome: () => void;
}

function ThemeIcon({ theme }: { theme: ThemeMode }) {
  if (theme === 'dark') {
    return (
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
      </svg>
    );
  }
  if (theme === 'light') {
    return (
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="4.5" />
        <line x1="12" y1="1.5" x2="12" y2="3.5" />
        <line x1="12" y1="20.5" x2="12" y2="22.5" />
        <line x1="4.2" y1="4.2" x2="5.6" y2="5.6" />
        <line x1="18.4" y1="18.4" x2="19.8" y2="19.8" />
        <line x1="1.5" y1="12" x2="3.5" y2="12" />
        <line x1="20.5" y1="12" x2="22.5" y2="12" />
        <line x1="4.2" y1="19.8" x2="5.6" y2="18.4" />
        <line x1="18.4" y1="5.6" x2="19.8" y2="4.2" />
      </svg>
    );
  }
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="2.5" y="3.5" width="19" height="13" rx="2" ry="2" />
      <line x1="8" y1="21" x2="16" y2="21" />
      <line x1="12" y1="16.5" x2="12" y2="21" />
    </svg>
  );
}

export function Topbar({
  tool,
  isHome,
  jobCount,
  runningCount,
  theme,
  onToggleTheme,
  onOpenSearch,
  onOpenShortcuts,
  onSelectHome,
}: TopbarProps) {
  const domain = domainOfTool(tool);

  return (
    <header className="topbar">
      <div className="topbar-inner">
        <div className="topbar-left">
          <button type="button" className="topbar-home-btn" onClick={onSelectHome} aria-label="All tools" title="All tools">
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M3 10.5 12 3l9 7.5" />
              <path d="M5 9.5V21h14V9.5" />
            </svg>
          </button>
          <span className="topbar-sep" aria-hidden="true">/</span>
          {isHome ? (
            <span className="topbar-title">All tools</span>
          ) : (
            <>
              <span className="topbar-domain">{domain.shortLabel}</span>
              <span className="topbar-sep" aria-hidden="true">/</span>
              <span className="topbar-title">{TOOLS[tool].label}</span>
            </>
          )}
        </div>

        <div className="topbar-right">
          {jobCount > 0 && (
            <span className="topbar-jobs" title={`${jobCount} files in the queue`}>
              <span className={`topbar-jobs-dot${runningCount > 0 ? ' is-busy' : ''}`} aria-hidden="true" />
              {runningCount > 0 ? `${runningCount} running` : `${jobCount} in queue`}
            </span>
          )}

          <button
            type="button"
            className="topbar-search-btn"
            onClick={onOpenSearch}
            title="Search tools and presets (Ctrl+K)"
            aria-label="Search tools and presets"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <circle cx="11" cy="11" r="7.5" />
              <line x1="20.5" y1="20.5" x2="16.2" y2="16.2" />
            </svg>
            <span className="topbar-search-text">Search</span>
            <kbd>Ctrl K</kbd>
          </button>

          <button type="button" className="icon-btn" onClick={onOpenShortcuts} aria-label="Keyboard shortcuts" title="Keyboard shortcuts (?)">
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <circle cx="12" cy="12" r="9" />
              <path d="M9.4 9a2.7 2.7 0 0 1 5.2 1c0 1.8-2.6 2.4-2.6 3.5" />
              <line x1="12" y1="17" x2="12.01" y2="17" />
            </svg>
          </button>

          <button
            type="button"
            className="icon-btn"
            onClick={onToggleTheme}
            aria-label={`Theme: ${theme}. Click to change.`}
            title={`Theme: ${theme}`}
          >
            <ThemeIcon theme={theme} />
          </button>
        </div>
      </div>
    </header>
  );
}
