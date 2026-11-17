import { useEffect, useId, useRef, useState, type DragEvent } from 'react';
import { batchStatus, FORMATS, resolveOptions, TOOLS, type Kind, type Options, type ToolId } from '@refract/core';
import { JobCard, size } from './JobCard.tsx';
import { OptionsForm } from './OptionsForm.tsx';
import { useJobs, type JobView } from './useJobs.ts';
import { CommandPalette, POPULAR_PRESETS, type Preset } from './CommandPalette.tsx';
import { ShortcutsModal } from './ShortcutsModal.tsx';
import { UseCasesSection } from './UseCasesSection.tsx';
import { FaqSection } from './FaqSection.tsx';
import { createSampleAudio, createSampleImage, createSamplePdf } from './sampleFiles.ts';

type DomainId = 'image' | 'pdf' | 'audio';

function DomainIcon({ domain }: { domain: DomainId }) {
  if (domain === 'pdf') {
    return (
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
        <polyline points="14 2 14 8 20 8" />
        <line x1="16" y1="13" x2="8" y2="13" />
        <line x1="16" y1="17" x2="8" y2="17" />
      </svg>
    );
  }
  if (domain === 'audio') {
    return (
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M9 18V5l12-2v13" />
        <circle cx="6" cy="18" r="3" />
        <circle cx="18" cy="16" r="3" />
      </svg>
    );
  }
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
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

interface Recipe {
  id: string;
  label: string;
  tool: ToolId;
  options?: Options;
}

const RECIPES_BY_DOMAIN: Record<DomainId, Recipe[]> = {
  image: [
    { id: 'heic-to-jpg', label: 'HEIC to JPEG', tool: 'convert-image', options: { format: 'jpeg', quality: 85 } },
    { id: 'png-to-webp', label: 'PNG to WebP (lossless)', tool: 'convert-image', options: { format: 'webp', quality: 90 } },
    { id: 'batch-compress', label: 'Batch compress (<500 KB)', tool: 'compress-image', options: { maxBytes: 500_000 } },
    { id: 'resize-half', label: 'Resize to 1200px width', tool: 'resize-image', options: { width: 1200 } },
  ],
  pdf: [
    { id: 'merge-pdf', label: 'Merge multiple PDFs', tool: 'merge-pdf' },
    { id: 'compress-pdf', label: 'Compress PDF (QPDF)', tool: 'compress-pdf' },
    { id: 'pdf-to-img', label: 'PDF to images (PNG)', tool: 'pdf-to-images' },
    { id: 'img-to-pdf', label: 'Images to single PDF', tool: 'images-to-pdf' },
  ],
  audio: [
    { id: 'to-mp3', label: 'Convert to MP3 (192 kbps)', tool: 'convert-audio', options: { format: 'mp3', bitrate: '192k' } },
    { id: 'to-wav', label: 'Extract WAV (PCM)', tool: 'convert-audio', options: { format: 'wav' } },
    { id: 'trim-silence', label: 'Trim audio segment', tool: 'trim-audio' },
  ],
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
  const { jobs, toast, addFiles, cancel, retry, reRunAll, answerPassword, saveAllUrl, clearJobs, removeJob } = useJobs();
  const [tool, setTool] = useState<ToolId>(toolFromHash);
  const [options, setOptions] = useState<Options>(() => loadOptions(toolFromHash()));
  const [over, setOver] = useState(false);
  const [allUrl, setAllUrl] = useState<string>();
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [sampleLoading, setSampleLoading] = useState(false);
  const [activePresetNotice, setActivePresetNotice] = useState<string | null>(null);
  const [isDraggingViewport, setIsDraggingViewport] = useState(false);
  const dragCounter = useRef(0);

  const [theme, setTheme] = useState<ThemeMode>(() => {
    try {
      return (localStorage.getItem('refract.theme') as ThemeMode) || 'system';
    } catch {
      return 'system';
    }
  });

  const inputId = useId();
  const t = TOOLS[tool];
  const currentDomain = (DOMAIN_GROUPS.find((g) => g.tools.includes(tool))?.id ?? 'image') as DomainId;

  // Sync hash changes
  useEffect(() => {
    const onHash = () => {
      const next = toolFromHash();
      setTool(next);
      setOptions(loadOptions(next));
    };
    addEventListener('hashchange', onHash);
    return () => removeEventListener('hashchange', onHash);
  }, []);

  // Theme synchronization
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

  // Global Full-Viewport Drag & Drop Listener
  useEffect(() => {
    const onDragEnter = (e: globalThis.DragEvent) => {
      e.preventDefault();
      dragCounter.current += 1;
      if (e.dataTransfer?.types?.includes('Files')) {
        setIsDraggingViewport(true);
      }
    };

    const onDragLeave = (e: globalThis.DragEvent) => {
      e.preventDefault();
      dragCounter.current -= 1;
      if (dragCounter.current <= 0) {
        dragCounter.current = 0;
        setIsDraggingViewport(false);
      }
    };

    const onDragOver = (e: globalThis.DragEvent) => {
      e.preventDefault();
    };

    const onGlobalDrop = (e: globalThis.DragEvent) => {
      e.preventDefault();
      dragCounter.current = 0;
      setIsDraggingViewport(false);
      if (e.dataTransfer?.files?.length) {
        add([...e.dataTransfer.files]);
      }
    };

    window.addEventListener('dragenter', onDragEnter);
    window.addEventListener('dragleave', onDragLeave);
    window.addEventListener('dragover', onDragOver);
    window.addEventListener('drop', onGlobalDrop);

    return () => {
      window.removeEventListener('dragenter', onDragEnter);
      window.removeEventListener('dragleave', onDragLeave);
      window.removeEventListener('dragover', onDragOver);
      window.removeEventListener('drop', onGlobalDrop);
    };
  }, [tool, options]);

  // Clipboard paste support & Global shortcuts
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

    const onKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      const isTyping = target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT');

      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setPaletteOpen((prev) => !prev);
      } else if (!isTyping && e.key === '?') {
        e.preventDefault();
        setShortcutsOpen((prev) => !prev);
      } else if (!isTyping && e.key === '1') {
        selectTool(DOMAIN_GROUPS[0]!.tools[0]!);
      } else if (!isTyping && e.key === '2') {
        selectTool(DOMAIN_GROUPS[1]!.tools[0]!);
      } else if (!isTyping && e.key === '3') {
        selectTool(DOMAIN_GROUPS[2]!.tools[0]!);
      }
    };

    window.addEventListener('paste', onPaste);
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('paste', onPaste);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [tool, options]);

  const selectTool = (targetTool: ToolId) => {
    location.hash = `#${targetTool}`;
    setTool(targetTool);
    setOptions(loadOptions(targetTool));
    setActivePresetNotice(null);
  };

  const applyPreset = (preset: Preset) => {
    location.hash = `#${preset.tool}`;
    setTool(preset.tool);
    const defaultOpts = loadOptions(preset.tool);
    const merged: Options = preset.options ? { ...defaultOpts, ...preset.options } : defaultOpts;
    setOptions(merged);
    saveOptions(preset.tool, resolveOptions(TOOLS[preset.tool].options, merged));
    setActivePresetNotice(`Configured: ${preset.title}`);
  };

  const applyRecipe = (recipe: Recipe) => {
    location.hash = `#${recipe.tool}`;
    setTool(recipe.tool);
    const defaultOpts = loadOptions(recipe.tool);
    const merged: Options = recipe.options ? { ...defaultOpts, ...recipe.options } : defaultOpts;
    setOptions(merged);
    saveOptions(recipe.tool, resolveOptions(TOOLS[recipe.tool].options, merged));
    setActivePresetNotice(`Recipe activated: ${recipe.label}`);
  };

  const applyWorkflow = (targetTool: ToolId, customOptions?: Options) => {
    location.hash = `#${targetTool}`;
    setTool(targetTool);
    const defaultOpts = loadOptions(targetTool);
    const merged: Options = customOptions ? { ...defaultOpts, ...customOptions } : defaultOpts;
    setOptions(merged);
    saveOptions(targetTool, resolveOptions(TOOLS[targetTool].options, merged));
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

  const loadSample = async (kind: Kind) => {
    try {
      setSampleLoading(true);
      let file: File;
      if (kind === 'image') {
        file = await createSampleImage();
      } else if (kind === 'pdf') {
        file = await createSamplePdf();
      } else {
        file = createSampleAudio();
      }
      add([file]);
    } finally {
      setSampleLoading(false);
    }
  };

  // Hardware and queue statistics
  const activeJobsCount = jobs.filter((j) => j.state === 'Running' || j.state === 'Probing' || j.state === 'Verifying').length;
  const hardwareThreads = typeof navigator !== 'undefined' ? navigator.hardwareConcurrency || 8 : 8;
  const status = jobs.length ? batchStatus(jobs.map((j) => j.state)) : null;
  const savedJobs = jobs.filter((j) => j.state === 'Saved');
  const savedCount = savedJobs.length;

  const completedWithSizes = savedJobs.filter((j) => j.inputSize && j.outputSize);
  const totalInputBytes = completedWithSizes.reduce((acc, j) => acc + (j.inputSize || 0), 0);
  const totalOutputBytes = completedWithSizes.reduce((acc, j) => acc + (j.outputSize || 0), 0);
  const savedPercentage = totalInputBytes > 0 ? Math.round(((totalInputBytes - totalOutputBytes) / totalInputBytes) * 100) : 0;
  const totalSavedBytes = Math.max(0, totalInputBytes - totalOutputBytes);

  // Engine name
  const engineLabel =
    currentDomain === 'image'
      ? 'libwebp / mozjpeg / libpng WASM'
      : currentDomain === 'pdf'
      ? 'pdf-lib / qpdf WASM'
      : 'ffmpeg-wasm / PCM WebAudio';

  // Memory estimation
  const memoryEstimateMb = Math.min(
    512,
    Math.max(38, Math.round((jobs.length * 9.5) + (totalInputBytes / 1024 / 1024 * 0.4) + 38))
  );

  return (
    <div className="app-shell">
      {/* Full-Viewport Drag & Drop Overlay */}
      {isDraggingViewport && (
        <div className="full-viewport-drag-overlay" aria-hidden="true">
          <div className="drag-overlay-box">
            <span className="drag-crosshair tl">+</span>
            <span className="drag-crosshair tr">+</span>
            <span className="drag-crosshair bl">+</span>
            <span className="drag-crosshair br">+</span>

            <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" className="drag-overlay-icon">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
              <polyline points="17 8 12 3 7 8" />
              <line x1="12" y1="3" x2="12" y2="15" />
            </svg>
            <h2 className="drag-overlay-title">Drop files anywhere to convert</h2>
            <p className="drag-overlay-sub">Processing 100% on-device in isolated WebAssembly memory</p>
          </div>
        </div>
      )}

      {/* Topbar: Wordmark, Telemetry Pill, Segmented Domain Selector, Shortcuts */}
      <header className="topbar">
        <div className="topbar-inner">
          <div className="topbar-left">
            <a href="#convert-image" className="wordmark" title="Refract — Local-First File Converter">
              <span className="wordmark-icon" aria-hidden="true">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                  <polygon points="12 2 22 20 2 20" />
                  <line x1="12" y1="2" x2="12" y2="20" strokeDasharray="2 2" strokeWidth="1.2" />
                </svg>
              </span>
              <span className="wordmark-text">refract</span>
            </a>

            <div className="live-status-pill" title="Local WebAssembly sandbox. Files never leave your machine.">
              <span className={`status-dot${activeJobsCount > 0 ? ' is-busy' : ''}`} aria-hidden="true" />
              <span>
                {activeJobsCount > 0
                  ? `WASM: ${activeJobsCount} active`
                  : 'WASM: ready'}
              </span>
            </div>
          </div>

          {/* Center Domain Segmented Controller */}
          <div className="domain-segmented-control" role="tablist" aria-label="Tool domains">
            {DOMAIN_GROUPS.map((g) => {
              const isActive = currentDomain === g.id;
              return (
                <button
                  key={g.id}
                  type="button"
                  role="tab"
                  aria-selected={isActive}
                  className={`segment-btn${isActive ? ' is-active' : ''}`}
                  onClick={() => {
                    if (!isActive) {
                      selectTool(g.tools[0]!);
                    }
                  }}
                >
                  <DomainIcon domain={g.id} />
                  <span>{g.label}</span>
                  <span className="segment-count">{g.tools.length}</span>
                </button>
              );
            })}
          </div>

          <div className="topbar-right">
            <button
              type="button"
              className="topbar-search-btn"
              onClick={() => setPaletteOpen(true)}
              title="Search tools and presets (⌘K / Ctrl+K)"
              aria-label="Search tools"
            >
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <circle cx="11" cy="11" r="8" />
                <line x1="21" y1="21" x2="16.65" y2="16.65" />
              </svg>
              <span className="search-btn-text">Search</span>
              <kbd className="search-kbd">⌘K</kbd>
            </button>

            <button
              type="button"
              className="icon-btn-subtle"
              onClick={() => setShortcutsOpen(true)}
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

      {/* Semantic Accessible Navigation Bar for all tools (always in DOM for tests & deep links) */}
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
                      selectTool(id);
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

      {/* Main Experience Shell */}
      <main className="shell">
        {/* Semantic Accessible Heading for Active Tool */}
        <h1 id="tool-title" className="visually-hidden">{t.label}</h1>

        {/* Active Preset Notification Banner */}
        {activePresetNotice && (
          <div className="preset-notice-bar" role="status">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="notice-icon">
              <polyline points="20 6 9 17 4 12" />
            </svg>
            <span>{activePresetNotice}</span>
            <button
              type="button"
              className="notice-dismiss"
              onClick={() => setActivePresetNotice(null)}
              aria-label="Dismiss notice"
            >
              ×
            </button>
          </div>
        )}

        {/* ============================================================== */}
        {/* STATE A: EMPTY / READY STATE (when jobs.length === 0)         */}
        {/* Dropzone is the HERO: 01 / Stage, 02 / Target pipeline, etc.  */}
        {/* ============================================================== */}
        {jobs.length === 0 ? (
          <div className="utility-ready-workspace">
            {/* 01 / Stage Hero Dropzone */}
            <section className="stage-section" aria-labelledby="stage-heading">
              <div className="section-label-row">
                <span className="section-step-num">01</span>
                <span className="section-step-slash">/</span>
                <h2 id="stage-heading" className="section-step-title">Stage</h2>
                <span className="section-target-badge">{t.label}</span>
              </div>

              <label
                htmlFor={inputId}
                className={`dropzone utility-stage-dropzone${over ? ' over' : ''}`}
                data-testid="dropzone"
                onDragOver={(e) => {
                  e.preventDefault();
                  setOver(true);
                }}
                onDragLeave={() => setOver(false)}
                onDrop={onDrop}
              >
                {/* 4 Corner Crosshairs */}
                <span className="crosshair ch-tl">+</span>
                <span className="crosshair ch-tr">+</span>
                <span className="crosshair ch-bl">+</span>
                <span className="crosshair ch-br">+</span>

                <div className="stage-drop-body">
                  <div className="stage-drop-icon" aria-hidden="true">
                    <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                      <polyline points="17 8 12 3 7 8" />
                      <line x1="12" y1="3" x2="12" y2="15" />
                    </svg>
                  </div>

                  <strong className="stage-drop-title">
                    Drop {t.accepts === 'image' ? 'images' : t.accepts === 'pdf' ? 'PDF documents' : 'audio tracks'} or folders
                  </strong>
                  <span className="stage-drop-subtitle">
                    Click to browse  ·  Paste clipboard ⌘V
                  </span>

                  {/* Format Pills */}
                  <div className="stage-format-pills">
                    {FORMAT_HINTS[t.accepts]?.map((fmt) => (
                      <span key={fmt} className="stage-format-chip">
                        {fmt}
                      </span>
                    ))}
                  </div>

                  {/* Quick Start Buttons Directly in the Dropzone */}
                  <div className="stage-quick-start" onClick={(e) => e.stopPropagation()}>
                    <span className="stage-quick-label">Quick start:</span>
                    {t.accepts === 'image' ? (
                      <>
                        <button
                          type="button"
                          className="stage-sample-btn"
                          disabled={sampleLoading}
                          onClick={() => loadSample('image')}
                        >
                          {sampleLoading ? 'Generating…' : 'Sample PNG canvas'}
                        </button>
                        <button
                          type="button"
                          className="stage-sample-btn"
                          disabled={sampleLoading}
                          onClick={() => {
                            applyRecipe(RECIPES_BY_DOMAIN.image[0]!);
                            void loadSample('image');
                          }}
                        >
                          Sample HEIC preset
                        </button>
                      </>
                    ) : t.accepts === 'pdf' ? (
                      <button
                        type="button"
                        className="stage-sample-btn"
                        disabled={sampleLoading}
                        onClick={() => loadSample('pdf')}
                      >
                        {sampleLoading ? 'Generating…' : 'Sample 2-page PDF'}
                      </button>
                    ) : (
                      <button
                        type="button"
                        className="stage-sample-btn"
                        disabled={sampleLoading}
                        onClick={() => loadSample('audio')}
                      >
                        {sampleLoading ? 'Generating…' : 'Sample WAV synth'}
                      </button>
                    )}
                  </div>
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

            {/* 02 / Target Pipeline Persistent Configuration Dock */}
            <section className="pipeline-section" aria-labelledby="pipeline-heading">
              <div className="section-label-row">
                <span className="section-step-num">02</span>
                <span className="section-step-slash">/</span>
                <h2 id="pipeline-heading" className="section-step-title">
                  Target pipeline ({DOMAIN_GROUPS.find((g) => g.id === currentDomain)?.label ?? 'Image'})
                </h2>
                <span className="pipeline-tool-desc">{t.blurb}</span>
              </div>

              <div className="pipeline-dock">
                <OptionsForm
                  specs={t.options}
                  values={options}
                  onChange={changeOptions}
                  onReset={() => changeOptions(resolveOptions(t.options))}
                />
              </div>
            </section>

            {/* 03 / Quick Recipes Contextual Dock */}
            <section className="recipes-section" aria-labelledby="recipes-heading">
              <div className="section-label-row">
                <span className="section-step-num">03</span>
                <span className="section-step-slash">/</span>
                <h2 id="recipes-heading" className="section-step-title">Quick recipes</h2>
                <span className="recipes-domain-tag">({DOMAIN_GROUPS.find((g) => g.id === currentDomain)?.label})</span>
              </div>

              <div className="recipes-grid">
                {RECIPES_BY_DOMAIN[currentDomain]?.map((r) => (
                  <button
                    key={r.id}
                    type="button"
                    className="recipe-card-btn"
                    onClick={() => applyRecipe(r)}
                  >
                    <span className="recipe-btn-title">{r.label}</span>
                    <span className="recipe-btn-arrow">→</span>
                  </button>
                ))}
              </div>
            </section>
          </div>
        ) : (
          /* ============================================================== */
          /* STATE B: ACTIVE PROCESSING / BATCH STATE (jobs.length > 0)     */
          /* Top drop bar + Split View (Queue Left, Output Settings Right)  */
          /* ============================================================== */
          <div className="utility-active-workspace">
            {/* Top Compact Drop Bar */}
            <div className="queue-top-action-bar">
              <label
                htmlFor={inputId}
                className={`queue-top-dropbar${over ? ' over' : ''}`}
                data-testid="dropzone"
                onDragOver={(e) => {
                  e.preventDefault();
                  setOver(true);
                }}
                onDragLeave={() => setOver(false)}
                onDrop={onDrop}
              >
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <line x1="12" y1="5" x2="12" y2="19" />
                  <line x1="5" y1="12" x2="19" y2="12" />
                </svg>
                <span className="queue-dropbar-text">
                  Add more files or drop anywhere (⌘V)
                </span>
                <span className="queue-dropbar-badge">{t.accepts}</span>

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

              <div className="queue-top-actions">
                {savedCount > 0 && (
                  <button
                    type="button"
                    className="button clear-btn queue-clear-action"
                    onClick={clearJobs}
                    title="Clear finished jobs"
                  >
                    Clear Queue (✕)
                  </button>
                )}
              </div>
            </div>

            {/* Split Queue + Persistent Output Inspector */}
            <div className="queue-split-layout">
              {/* Left Column: Job Queue */}
              <div className="queue-main-col">
                <div className="queue-header-row">
                  <div className="queue-header-left">
                    <span className="queue-header-label">Queue</span>
                    <span className="queue-count-pill">({jobs.length} {jobs.length === 1 ? 'file' : 'files'})</span>
                    {status && (
                      <span className="queue-batch-status" data-batch={status}>
                        ● {status}
                      </span>
                    )}
                  </div>

                  {totalInputBytes > 0 && totalOutputBytes > 0 && (
                    <div className="queue-savings-badge">
                      <span className="savings-label">Saved:</span>
                      <span className={`savings-val ${savedPercentage > 0 ? 'is-good' : ''}`}>
                        {savedPercentage > 0 ? `-${savedPercentage}%` : `+${Math.abs(savedPercentage)}%`}
                      </span>
                      <span className="savings-detail">({size(totalSavedBytes)})</span>
                    </div>
                  )}
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
              </div>

              {/* Right Column: Persistent Output Settings Dock & Batch Downloader */}
              <div className="queue-inspector-col">
                <div className="inspector-dock-sticky">
                  <div className="inspector-header">
                    <div className="section-label-row">
                      <span className="section-step-num">02</span>
                      <span className="section-step-slash">/</span>
                      <h3 className="section-step-title">Output settings</h3>
                    </div>
                    <span className="inspector-tool-tag">{TOOLS[tool].label}</span>
                  </div>

                  {/* Persistent Target Configuration Form */}
                  <div className="inspector-options-body">
                    <OptionsForm
                      specs={t.options}
                      values={options}
                      onChange={changeOptions}
                      onReset={() => changeOptions(resolveOptions(t.options))}
                    />
                  </div>

                  {/* Batch Actions Box */}
                  <div className="inspector-download-box">
                    {savedCount > 1 ? (
                      allUrl ? (
                        <a
                          className="button primary inspector-zip-btn"
                          href={allUrl}
                          download="refract.zip"
                          onClick={() => setTimeout(() => setAllUrl(undefined), 0)}
                        >
                          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                            <polyline points="7 10 12 15 17 10" />
                            <line x1="12" y1="15" x2="12" y2="3" />
                          </svg>
                          <span>Download refract.zip</span>
                        </a>
                      ) : (
                        <button
                          type="button"
                          className="button primary inspector-zip-btn"
                          onClick={() => void saveAllUrl(jobs).then(setAllUrl)}
                        >
                          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                            <polyline points="7 10 12 15 17 10" />
                            <line x1="12" y1="15" x2="12" y2="3" />
                          </svg>
                          <span>Save all as ZIP</span>
                        </button>
                      )
                    ) : savedCount === 1 && savedJobs[0]?.url ? (
                      <a
                        className="button primary inspector-zip-btn"
                        href={savedJobs[0].url}
                        download={savedJobs[0].outputName}
                      >
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                          <polyline points="7 10 12 15 17 10" />
                          <line x1="12" y1="15" x2="12" y2="3" />
                        </svg>
                        <span>Download {savedJobs[0].outputName}</span>
                      </a>
                    ) : (
                      <div className="inspector-waiting-box">
                        <span>Waiting for jobs to finish encoding…</span>
                      </div>
                    )}

                    {/* Instant Re-Run with New Settings */}
                    <button
                      type="button"
                      className="button subtle-btn rerun-all-btn"
                      onClick={() => reRunAll(tool, resolveOptions(t.options, options))}
                      title="Re-run all files with updated settings"
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                        <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2" />
                      </svg>
                      <span>Re-process queue with current settings</span>
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Toast Status Notice */}
        <div role="status" aria-live="polite" className="toast">
          {toast}
        </div>

        {/* Supporting Architectural Reference & FAQ (kept accessible for reference below workspace) */}
        <UseCasesSection
          onSelectTool={selectTool}
          onApplyWorkflow={applyWorkflow}
        />

        <FaqSection />
      </main>

      {/* Functional Telemetry Status Bar */}
      <aside className="telemetry-bar" aria-label="Engine Telemetry">
        <div className="telemetry-bar-inner">
          <div className="telemetry-col">
            <svg className="telemetry-lock-icon" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
              <path d="M7 11V7a5 5 0 0 1 10 0v4" />
            </svg>
            <span className="telemetry-text">100% on-device sandbox · 0 bytes uploaded</span>
          </div>

          <div className="telemetry-col">
            <span className="telemetry-label">Threads:</span>
            <span className="telemetry-val">
              {activeJobsCount}/{hardwareThreads} WASM
            </span>
          </div>

          <div className="telemetry-col telemetry-col-engine">
            <span className="telemetry-label">Engine:</span>
            <span className="telemetry-val">{engineLabel}</span>
          </div>

          <div className="telemetry-col">
            <span className="telemetry-label">Memory:</span>
            <span className="telemetry-val">~{memoryEstimateMb} MB</span>
          </div>

          <div className="telemetry-col telemetry-col-version">
            <span className="telemetry-val">v1.0.0</span>
          </div>

          {jobs.length > 0 && (
            <div className="telemetry-col telemetry-action-col">
              <button
                type="button"
                className="telemetry-rerun-btn"
                onClick={() => reRunAll(tool, resolveOptions(t.options, options))}
                title="Re-run all jobs in memory"
              >
                <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67" />
                </svg>
                <span>Re-run queue</span>
              </button>
            </div>
          )}
        </div>
      </aside>

      {/* Quiet Utilitarian Footer */}
      <footer className="footer" aria-label="Page footer">
        <div className="footer-inner">
          <div className="footer-left">
            <div className="footer-brand">
              <span className="wordmark-icon" aria-hidden="true">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                  <polygon points="12 2 22 20 2 20" />
                </svg>
              </span>
              <strong className="footer-name">refract</strong>
            </div>
            <p className="footer-tagline">
              Open-source, local-first file converter. Powered by WebAssembly. No telemetry, no accounts, no ads.
            </p>
          </div>

          <div className="footer-right">
            <div className="footer-nav-col">
              <h4>Tools</h4>
              <a href="#convert-image" onClick={() => selectTool('convert-image')}>Images (3)</a>
              <a href="#merge-pdf" onClick={() => selectTool('merge-pdf')}>PDF (6)</a>
              <a href="#convert-audio" onClick={() => selectTool('convert-audio')}>Audio (2)</a>
            </div>

            <div className="footer-nav-col">
              <h4>Documentation</h4>
              <button type="button" className="footer-link-btn" onClick={() => setShortcutsOpen(true)}>
                Shortcuts (?)
              </button>
              <button type="button" className="footer-link-btn" onClick={() => setPaletteOpen(true)}>
                Command Palette (⌘K)
              </button>
              <a href="https://github.com/w1tneess/refract" target="_blank" rel="noopener noreferrer">
                GitHub
              </a>
            </div>
          </div>
        </div>

        <div className="footer-bottom">
          <p>© 2026 Refract Project. Released under GPL-3.0-or-later. Files never leave your browser.</p>
        </div>
      </footer>

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
        <button
          type="button"
          className="mobile-tab"
          onClick={() => setPaletteOpen(true)}
          aria-label="Search"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <circle cx="11" cy="11" r="8" />
            <line x1="21" y1="21" x2="16.65" y2="16.65" />
          </svg>
          <span className="mobile-tab-label">Search</span>
        </button>
      </nav>

      {/* Command Palette Modal */}
      <CommandPalette
        isOpen={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        onSelectTool={selectTool}
        onApplyPreset={applyPreset}
      />

      {/* Keyboard Shortcuts Modal */}
      <ShortcutsModal
        isOpen={shortcutsOpen}
        onClose={() => setShortcutsOpen(false)}
      />
    </div>
  );
}
