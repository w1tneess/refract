import { useEffect, useId, useState, type DragEvent } from 'react';
import { batchStatus, FORMATS, resolveOptions, TOOLS, type Kind, type Options, type ToolId } from '@refract/core';
import { JobCard } from './JobCard.tsx';
import { OptionsForm } from './OptionsForm.tsx';
import { useJobs, type JobView } from './useJobs.ts';

type DomainId = 'image' | 'pdf' | 'audio';

function DomainIcon({ domain }: { domain: DomainId }) {
  if (domain === 'pdf') {
    return (
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
        <polyline points="14 2 14 8 20 8" />
        <line x1="16" y1="13" x2="8" y2="13" />
        <line x1="16" y1="17" x2="8" y2="17" />
      </svg>
    );
  }
  if (domain === 'audio') {
    return (
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M9 18V5l12-2v13" />
        <circle cx="6" cy="18" r="3" />
        <circle cx="18" cy="16" r="3" />
      </svg>
    );
  }
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
      <circle cx="8.5" cy="8.5" r="1.5" />
      <polyline points="21 15 16 10 5 21" />
    </svg>
  );
}

const DOMAIN_GROUPS: { id: DomainId; label: string; tools: ToolId[] }[] = [
  { id: 'image', label: 'Images', tools: ['convert-image', 'compress-image', 'resize-image'] },
  { id: 'pdf', label: 'PDF', tools: ['merge-pdf', 'split-pdf', 'reorder-pdf', 'images-to-pdf', 'pdf-to-images', 'compress-pdf'] },
  { id: 'audio', label: 'Audio', tools: ['convert-audio', 'trim-audio'] },
];

const FORMAT_HINTS: Record<Kind, string[]> = {
  image: ['HEIC', 'JPEG', 'PNG', 'WebP'],
  pdf: ['PDF', 'Password unlock', 'Multi-page'],
  audio: ['MP3', 'WAV', 'M4A', 'OGG'],
};

const ACCEPT: Record<Kind, string> = Object.fromEntries(
  (['image', 'pdf', 'audio'] as Kind[]).map((k) => [
    k,
    Object.values(FORMATS)
      .filter((f) => f.kind === k)
      .flatMap((f) => [...f.ext.map((e) => `.${e}`), f.mime])
      .join(','),
  ]),
) as Record<Kind, string>;

const toolFromHash = (): ToolId => {
  const h = location.hash.slice(1) as ToolId;
  return h in TOOLS ? h : 'convert-image';
};

// Remembered options per tool: a per-viewer convenience, so storage failures are ignored.
const loadOptions = (tool: ToolId): Options => {
  try {
    return resolveOptions(TOOLS[tool].options, JSON.parse(localStorage.getItem(`refract.options.${tool}`) ?? '{}'));
  } catch {
    return resolveOptions(TOOLS[tool].options);
  }
};

const saveOptions = (tool: ToolId, o: Options) => {
  try {
    localStorage.setItem(`refract.options.${tool}`, JSON.stringify(o));
  } catch {
    /* private mode */
  }
};

type ThemeMode = 'system' | 'light' | 'dark';

export function App() {
  const { jobs, toast, addFiles, cancel, retry, answerPassword, saveAllUrl, clearJobs, removeJob } = useJobs();
  const [tool, setTool] = useState<ToolId>(toolFromHash);
  const [options, setOptions] = useState<Options>(() => loadOptions(toolFromHash()));
  const [over, setOver] = useState(false);
  const [allUrl, setAllUrl] = useState<string>();
  const [theme, setTheme] = useState<ThemeMode>(() => {
    try {
      return (localStorage.getItem('refract.theme') as ThemeMode) || 'system';
    } catch {
      return 'system';
    }
  });

  const inputId = useId();
  const t = TOOLS[tool];

  // Sync active domain with active tool
  const currentDomain = (DOMAIN_GROUPS.find((g) => g.tools.includes(tool))?.id ?? 'image') as DomainId;

  useEffect(() => {
    const onHash = () => {
      const next = toolFromHash();
      setTool(next);
      setOptions(loadOptions(next));
    };
    addEventListener('hashchange', onHash);
    return () => removeEventListener('hashchange', onHash);
  }, []);

  // Theme application and synchronization
  useEffect(() => {
    try {
      localStorage.setItem('refract.theme', theme);
    } catch {}

    const apply = () => {
      const isDark =
        theme === 'dark' ||
        (theme === 'system' && window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches);
      if (isDark) {
        document.documentElement.setAttribute('data-theme', 'dark');
        document.documentElement.classList.add('dark');
      } else {
        document.documentElement.setAttribute('data-theme', 'light');
        document.documentElement.classList.remove('dark');
      }
    };

    apply();

    if (theme === 'system') {
      const media = window.matchMedia('(prefers-color-scheme: dark)');
      media.addEventListener('change', apply);
      return () => media.removeEventListener('change', apply);
    }
  }, [theme]);

  const toggleTheme = () => {
    setTheme((curr) => (curr === 'system' ? 'light' : curr === 'light' ? 'dark' : 'system'));
  };

  const changeOptions = (o: Options) => {
    setOptions(o);
    saveOptions(tool, resolveOptions(t.options, o));
  };

  const add = (files: File[]) => addFiles(tool, resolveOptions(t.options, options), files);

  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    setOver(false);
    add([...e.dataTransfer.files]);
  };

  // Clipboard paste support (Ctrl+V / Cmd+V)
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT')) {
        return;
      }
      const files = Array.from(e.clipboardData?.files ?? []);
      if (files.length > 0) {
        e.preventDefault();
        add(files);
      }
    };
    window.addEventListener('paste', onPaste);
    return () => window.removeEventListener('paste', onPaste);
  }, [tool, options]);

  const selectTool = (targetTool: ToolId) => {
    location.hash = `#${targetTool}`;
    setTool(targetTool);
    setOptions(loadOptions(targetTool));
  };

  const useInTool = (targetTool: ToolId, job: JobView) => {
    if (!job.outputs?.length) return;
    const newFiles = job.outputs.map((o) => new File([o.bytes as BlobPart], o.name, { type: FORMATS[o.format].mime }));
    location.hash = `#${targetTool}`;
    setTool(targetTool);
    const targetOptions = loadOptions(targetTool);
    setOptions(targetOptions);
    addFiles(targetTool, resolveOptions(TOOLS[targetTool].options, targetOptions), newFiles);
  };

  const status = jobs.length ? batchStatus(jobs.map((j) => j.state)) : null;
  const saved = jobs.filter((j) => j.state === 'Saved').length;

  return (
    <div className="app-shell">
      {/* Quiet, Crafted Topbar */}
      <header className="topbar">
        <div className="topbar-inner">
          <div className="topbar-left">
            <a href="#convert-image" className="wordmark" title="Refract">
              <span className="wordmark-icon" aria-hidden="true">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                  <polygon points="12 2 22 20 2 20" />
                  <line x1="12" y1="2" x2="12" y2="20" strokeDasharray="2 2" strokeWidth="1.2" />
                </svg>
              </span>
              <span className="wordmark-text">refract</span>
            </a>

            {/* Domain Tabs in Header */}
            <div className="domain-tabs" role="tablist" aria-label="Tool domains">
              {DOMAIN_GROUPS.map((g) => {
                const isActive = currentDomain === g.id;
                return (
                  <button
                    key={g.id}
                    type="button"
                    role="tab"
                    aria-selected={isActive}
                    className={`domain-tab${isActive ? ' active' : ''}`}
                    onClick={() => {
                      if (!isActive) {
                        selectTool(g.tools[0]!);
                      }
                    }}
                  >
                    <DomainIcon domain={g.id} />
                    <span>{g.label}</span>
                    <span className="domain-count">{g.tools.length}</span>
                  </button>
                );
              })}
            </div>
          </div>

          <div className="topbar-right">
            <div className="local-indicator" title="Files never leave your machine. Processed locally via WebAssembly.">
              <span className="indicator-dot" aria-hidden="true" />
              <span>Local WebAssembly</span>
            </div>

            <button
              type="button"
              className="theme-toggle"
              onClick={toggleTheme}
              aria-label={`Theme: ${theme}. Click to change.`}
              title={`Theme: ${theme}`}
            >
              {theme === 'dark' ? (
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
                </svg>
              ) : theme === 'light' ? (
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
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
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
                  <line x1="8" y1="21" x2="16" y2="21" />
                  <line x1="12" y1="17" x2="12" y2="21" />
                </svg>
              )}
            </button>
          </div>
        </div>
      </header>

      {/* Main 840px Shell */}
      <main className="shell">
        {/* Semantic Accessible Navigation Bar */}
        <nav aria-label="Tools" className="tools">
          {DOMAIN_GROUPS.map((g) => (
            <div key={g.label} className={`tool-group${g.id === currentDomain ? ' is-active' : ''}`}>
              <h2>{g.label}</h2>
              <ul>
                {g.tools.map((id) => (
                  <li key={id}>
                    <a
                      href={`#${id}`}
                      aria-current={id === tool ? 'page' : undefined}
                      className={id === tool ? 'active' : ''}
                      onClick={() => {
                        setTool(id);
                        setOptions(loadOptions(id));
                      }}
                    >
                      {TOOLS[id].label}
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </nav>

        {/* Hero Section for Current Tool */}
        <section aria-labelledby="tool-title" className="tool-workspace">
          <header className="workspace-header">
            <h1 id="tool-title">{t.label}</h1>
            <p className="blurb">{t.blurb}</p>
          </header>

          {/* Options Panel */}
          <OptionsForm
            specs={t.options}
            values={options}
            onChange={changeOptions}
            onReset={() => changeOptions(resolveOptions(t.options))}
          />

          {/* Dropzone with Drag, Click & Paste */}
          <label
            htmlFor={inputId}
            className={`dropzone${over ? ' over' : ''}`}
            data-testid="dropzone"
            onDragOver={(e) => {
              e.preventDefault();
              setOver(true);
            }}
            onDragLeave={() => setOver(false)}
            onDrop={onDrop}
          >
            <div className="dropzone-icon" aria-hidden="true">
              <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                <polyline points="17 8 12 3 7 8" />
                <line x1="12" y1="3" x2="12" y2="15" />
              </svg>
            </div>
            <strong className="dropzone-title">
              Drop {t.accepts === 'image' ? 'images' : t.accepts === 'pdf' ? 'PDFs' : 'audio files'} here
            </strong>
            <span className="dropzone-subtitle">
              or click to choose{t.inputs === 'many' ? ' (files are combined in order)' : ''} · Ctrl+V to paste
            </span>
            <div className="dropzone-tags">
              {FORMAT_HINTS[t.accepts]?.map((fmt) => (
                <span key={fmt} className="format-tag">
                  {fmt}
                </span>
              ))}
            </div>
            <input
              id={inputId}
              className="visually-hidden"
              type="file"
              multiple
              accept={ACCEPT[t.accepts]}
              onChange={(e) => {
                add([...(e.target.files ?? [])]);
                e.target.value = '';
              }}
            />
          </label>
        </section>

        {/* Toast Status Notice */}
        <div role="status" aria-live="polite" className="toast">
          {toast}
        </div>

        {/* 3-Step Guided Steps when Queue is Empty */}
        {jobs.length === 0 && (
          <section className="steps" aria-label="How Refract works">
            <div className="steps-grid">
              <div className="step-card">
                <span className="step-num">01</span>
                <h3 className="step-heading">Drop or paste</h3>
                <p className="step-text">Choose local files or paste an image from the clipboard. Files stay on your machine.</p>
              </div>
              <div className="step-card">
                <span className="step-num">02</span>
                <h3 className="step-heading">Adjust options</h3>
                <p className="step-text">Set target size, pixel dimensions, page sequence, or audio bitrates before processing.</p>
              </div>
              <div className="step-card">
                <span className="step-num">03</span>
                <h3 className="step-heading">Save or transfer</h3>
                <p className="step-text">Download outputs directly or send them to another tool without re-uploading.</p>
              </div>
            </div>
          </section>
        )}

        {/* Jobs Panel & Queue */}
        {jobs.length > 0 && (
          <section className="queue-section" aria-label="Job queue">
            <div className="panel-head batch-row">
              <div className="panel-title-area">
                <h2 className="panel-title">Jobs ({jobs.length})</h2>
                {status && (
                  <p className="batch" data-batch={status}>
                    Batch: {status}
                  </p>
                )}
              </div>

              <div className="panel-actions">
                {saved > 0 && (
                  <button type="button" className="button clear-btn" onClick={clearJobs} title="Clear finished jobs">
                    Clear finished
                  </button>
                )}
                {saved > 1 &&
                  (allUrl ? (
                    <a
                      className="button primary"
                      href={allUrl}
                      download="refract.zip"
                      onClick={() => setTimeout(() => setAllUrl(undefined), 0)}
                    >
                      Download refract.zip
                    </a>
                  ) : (
                    <button type="button" className="button primary" onClick={() => void saveAllUrl(jobs).then(setAllUrl)}>
                      Save all as ZIP
                    </button>
                  ))}
              </div>
            </div>

            <ul className="jobs" aria-label="Jobs">
              {jobs.map((j) => (
                <JobCard
                  key={j.id}
                  job={j}
                  onCancel={() => cancel(j.id)}
                  onRetry={() => retry(j.id)}
                  onPassword={(pw, all) => answerPassword(j.id, pw, all)}
                  onUseInTool={useInTool}
                  onRemove={() => removeJob(j.id)}
                />
              ))}
            </ul>
          </section>
        )}
      </main>

      {/* Mobile Bottom Navigation Bar */}
      <nav className="mobile-bottom-bar" aria-label="Mobile navigation">
        {DOMAIN_GROUPS.map((g) => {
          const isActive = currentDomain === g.id;
          return (
            <button
              key={g.id}
              type="button"
              className={`mobile-tab${isActive ? ' active' : ''}`}
              onClick={() => {
                if (!isActive) {
                  selectTool(g.tools[0]!);
                }
              }}
            >
              <DomainIcon domain={g.id} />
              <span className="mobile-tab-label">{g.label}</span>
            </button>
          );
        })}
      </nav>
    </div>
  );
}
