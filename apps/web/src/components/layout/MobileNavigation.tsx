import type { ToolId } from '@refract/core';
import { DOMAINS, type DomainId } from '../../config/domains.ts';
import { DomainIcon } from './DomainIcon.tsx';

interface MobileNavigationProps {
  currentDomain: DomainId;
  isHome: boolean;
  onSelectTool: (tool: ToolId) => void;
  onSelectHome: () => void;
  onOpenSearch: () => void;
}

export function MobileNavigation({
  currentDomain,
  isHome,
  onSelectTool,
  onSelectHome,
  onOpenSearch,
}: MobileNavigationProps) {
  return (
    <nav className="mobile-bottom-bar" aria-label="Mobile navigation">
      <button
        type="button"
        className={`mobile-tab${isHome ? ' is-active' : ''}`}
        onClick={onSelectHome}
        aria-label="All tools"
      >
        <DomainIcon domain="all" size={17} />
        <span className="mobile-tab-label">All</span>
      </button>

      {DOMAINS.map((domain) => {
        const active = !isHome && currentDomain === domain.id;
        return (
          <button
            key={domain.id}
            type="button"
            className={`mobile-tab${active ? ' is-active' : ''}`}
            onClick={() => onSelectTool(domain.tools[0]!)}
            aria-label={`${domain.label} tools`}
          >
            <DomainIcon domain={domain.id} size={17} />
            <span className="mobile-tab-label">{domain.shortLabel}</span>
          </button>
        );
      })}

      <button type="button" className="mobile-tab" onClick={onOpenSearch} aria-label="Search tools">
        <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <circle cx="11" cy="11" r="7.5" />
          <line x1="20.5" y1="20.5" x2="16.2" y2="16.2" />
        </svg>
        <span className="mobile-tab-label">Search</span>
      </button>
    </nav>
  );
}
