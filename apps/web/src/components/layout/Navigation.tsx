import { TOOLS, type ToolId } from '@refract/core';
import type { DomainId } from '../../config/recipes.ts';

export const DOMAIN_GROUPS: { id: DomainId; label: string; tools: ToolId[] }[] = [
  { id: 'image', label: 'Images', tools: ['convert-image', 'compress-image', 'resize-image'] },
  { id: 'pdf', label: 'PDF', tools: ['merge-pdf', 'split-pdf', 'reorder-pdf', 'images-to-pdf', 'pdf-to-images', 'compress-pdf'] },
  { id: 'audio', label: 'Audio', tools: ['convert-audio', 'trim-audio'] },
];

export function DomainIcon({ domain }: { domain: DomainId | 'all' }) {
  if (domain === 'pdf') {
    return (
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
        <polyline points="14 2 14 8 20 8" />
        <line x1="16" y1="13" x2="8" y2="13" />
        <line x1="16" y1="17" x2="8" y2="17" />
      </svg>
    );
  }
  if (domain === 'audio') {
    return (
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M9 18V5l12-2v13" />
        <circle cx="6" cy="18" r="3" />
        <circle cx="18" cy="16" r="3" />
      </svg>
    );
  }
  if (domain === 'all') {
    return (
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <rect x="3" y="3" width="7" height="7" />
        <rect x="14" y="3" width="7" height="7" />
        <rect x="14" y="14" width="7" height="7" />
        <rect x="3" y="14" width="7" height="7" />
      </svg>
    );
  }
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
      <circle cx="8.5" cy="8.5" r="1.5" />
      <polyline points="21 15 16 10 5 21" />
    </svg>
  );
}

interface NavigationProps {
  currentTool: ToolId;
  isHome: boolean;
  onSelectTool: (tool: ToolId) => void;
  onSelectHome: () => void;
}

export function Navigation({
  currentTool,
  isHome,
  onSelectTool,
  onSelectHome,
}: NavigationProps) {
  const currentDomain = (DOMAIN_GROUPS.find((g) => g.tools.includes(currentTool))?.id ?? 'image') as DomainId;

  return (
    <nav aria-label="Tools" className="tools">
      <div className="tools-inner">
        {/* Workspace Home Launcher */}
        <div className="tool-home-tab">
          <a
            href="#home"
            className={`tool-home-link${isHome ? ' is-active' : ''}`}
            onClick={(e) => {
              e.preventDefault();
              onSelectHome();
            }}
            title="All 12 Tools Overview"
          >
            <DomainIcon domain="all" />
            <span>Workspace</span>
          </a>
        </div>

        {/* Grouped Tool Domains */}
        <div className="tools-groups-row">
          {DOMAIN_GROUPS.map((g) => {
            const isDomainActive = !isHome && currentDomain === g.id;
            return (
              <div
                key={g.id}
                className={`tool-group${isDomainActive ? ' is-active' : ''}`}
                data-domain={g.id}
              >
                <button
                  type="button"
                  className="tool-group-header"
                  onClick={() => onSelectTool(g.tools[0]!)}
                  title={`Switch to ${g.label}`}
                >
                  <DomainIcon domain={g.id} />
                  <h2>{g.label}</h2>
                  <span className="tool-group-count">{g.tools.length}</span>
                </button>
                <ul className="tool-group-list">
                  {g.tools.map((id) => {
                    const isActive = !isHome && id === currentTool;
                    return (
                      <li key={id}>
                        <a
                          href={`#${id}`}
                          aria-current={isActive ? 'page' : undefined}
                          className={isActive ? 'active' : ''}
                          onClick={(e) => {
                            e.preventDefault();
                            onSelectTool(id);
                          }}
                        >
                          {TOOLS[id].label}
                        </a>
                      </li>
                    );
                  })}
                </ul>
              </div>
            );
          })}
        </div>
      </div>
    </nav>
  );
}
