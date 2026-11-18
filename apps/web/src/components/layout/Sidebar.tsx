import { TOOLS, type ToolId } from '@refract/core';
import { DOMAINS } from '../../config/domains.ts';
import { DomainIcon } from './DomainIcon.tsx';

interface SidebarProps {
  currentTool: ToolId;
  isHome: boolean;
  jobCount: number;
  onSelectTool: (tool: ToolId) => void;
  onSelectHome: () => void;
}

export function Sidebar({ currentTool, isHome, jobCount, onSelectTool, onSelectHome }: SidebarProps) {
  return (
    <aside className="sidebar" aria-label="Tool navigation">
      <div className="sidebar-inner">
        <a
          href="#home"
          className="sidebar-brand"
          title="Refract — local-first file tools"
          onClick={(e) => {
            e.preventDefault();
            onSelectHome();
          }}
        >
          <span className="brand-mark" aria-hidden="true">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <polygon points="12 3 21 19 3 19" />
              <line x1="12" y1="3" x2="12" y2="19" strokeDasharray="2 3" strokeWidth="1.3" />
            </svg>
          </span>
          <span className="brand-text">
            <span className="brand-name" translate="no">refract</span>
            <span className="brand-tag">Local file tools</span>
          </span>
        </a>

        <nav className="sidebar-nav" aria-label="Tools">
          <a
            href="#home"
            className={`sidebar-home-link${isHome ? ' is-active' : ''}`}
            aria-current={isHome ? 'page' : undefined}
            onClick={(e) => {
              e.preventDefault();
              onSelectHome();
            }}
          >
            <DomainIcon domain="all" size={15} />
            <span>All tools</span>
            {jobCount > 0 && (
              <>
                <span className="sidebar-count" aria-hidden="true">
                  {jobCount}
                </span>
                <span className="visually-hidden">
                  {jobCount === 1 ? '1 file' : `${jobCount} files`} in the queue
                </span>
              </>
            )}
          </a>

          {DOMAINS.map((domain) => (
            <div className="sidebar-group" key={domain.id}>
              <div className="sidebar-group-head">
                <DomainIcon domain={domain.id} size={13} />
                <span>{domain.label}</span>
                <span className="sidebar-group-count">{domain.tools.length}</span>
              </div>
              <ul className="sidebar-group-list">
                {domain.tools.map((id) => {
                  const active = !isHome && currentTool === id;
                  return (
                    <li key={id}>
                      <a
                        href={`#${id}`}
                        className={`sidebar-tool-link${active ? ' is-active' : ''}`}
                        aria-current={active ? 'page' : undefined}
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
          ))}
        </nav>

        <div className="sidebar-footer">
          <span className="sidebar-lock" aria-hidden="true">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
              <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
              <path d="M7 11V7a5 5 0 0 1 10 0v4" />
            </svg>
          </span>
          <p>
            Runs in your browser
            <br />
            Files never leave this device
          </p>
        </div>
      </div>
    </aside>
  );
}
