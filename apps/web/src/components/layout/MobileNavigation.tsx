import { DOMAIN_GROUPS, DomainIcon } from './Navigation.tsx';
import type { DomainId } from '../../config/recipes.ts';
import type { ToolId } from '@refract/core';

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
        className={`mobile-tab${isHome ? ' active' : ''}`}
        onClick={onSelectHome}
        aria-label="Home Workspace"
      >
        <DomainIcon domain="all" />
        <span className="mobile-tab-label">Home</span>
      </button>

      {DOMAIN_GROUPS.map((g) => {
        const isActive = !isHome && currentDomain === g.id;
        return (
          <button
            key={g.id}
            type="button"
            className={`mobile-tab${isActive ? ' active' : ''}`}
            onClick={() => onSelectTool(g.tools[0]!)}
            aria-label={`${g.label} tools`}
          >
            <DomainIcon domain={g.id} />
            <span className="mobile-tab-label">{g.label}</span>
          </button>
        );
      })}

      <button
        type="button"
        className="mobile-tab"
        onClick={onOpenSearch}
        aria-label="Search tools"
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <circle cx="11" cy="11" r="8" />
          <line x1="21" y1="21" x2="16.65" y2="16.65" />
        </svg>
        <span className="mobile-tab-label">Search</span>
      </button>
    </nav>
  );
}
