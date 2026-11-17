import { useEffect, useRef, useState, type DragEvent } from 'react';
import { FORMATS, resolveOptions, TOOLS, type Options, type ToolId } from '@refract/core';
import { useJobs, type JobView } from './useJobs.ts';
import { useTheme } from './hooks/useTheme.ts';
import { Topbar } from './components/layout/Topbar.tsx';
import { Navigation, DOMAIN_GROUPS } from './components/layout/Navigation.tsx';
import { MobileNavigation } from './components/layout/MobileNavigation.tsx';
import { TelemetryBar } from './components/layout/TelemetryBar.tsx';
import { Footer } from './components/layout/Footer.tsx';
import { HomePage } from './pages/HomePage.tsx';
import { ToolWorkspacePage } from './pages/ToolWorkspacePage.tsx';
import { CommandPalette, type Preset } from './CommandPalette.tsx';
import { ShortcutsModal } from './ShortcutsModal.tsx';
import { UseCasesSection } from './UseCasesSection.tsx';
import { FaqSection } from './FaqSection.tsx';
import type { DomainId, Recipe } from './config/recipes.ts';

const toolFromHash = (): ToolId => {
  const h = location.hash.slice(1) as ToolId;
  return h in TOOLS ? h : 'convert-image';
};

const isHomeFromHash = (): boolean => {
  const h = location.hash.slice(1);
  return h === 'home' || h === '';
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
    /* private mode / quota */
  }
};

export function App() {
  const { jobs, toast, addFiles, cancel, retry, reRunAll, answerPassword, saveAllUrl, clearJobs, removeJob } = useJobs();
  const { theme, toggleTheme } = useTheme();

  const [tool, setTool] = useState<ToolId>(toolFromHash);
  const [isHome, setIsHome] = useState<boolean>(() => isHomeFromHash() && jobs.length === 0);
  const [options, setOptions] = useState<Options>(() => loadOptions(toolFromHash()));
  const [allZipUrl, setAllZipUrl] = useState<string>();
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [activePresetNotice, setActivePresetNotice] = useState<string | null>(null);
  const [isDraggingViewport, setIsDraggingViewport] = useState(false);
  const dragCounter = useRef(0);

  const t = TOOLS[tool];
  const currentDomain = (DOMAIN_GROUPS.find((g) => g.tools.includes(tool))?.id ?? 'image') as DomainId;

  // Sync hash changes
  useEffect(() => {
    const onHash = () => {
      const h = location.hash.slice(1);
      if (h === 'home' || h === '') {
        setIsHome(true);
      } else if (h in TOOLS) {
        const next = h as ToolId;
        setTool(next);
        setOptions(loadOptions(next));
        setIsHome(false);
      }
    };
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, []);

  const changeOptions = (o: Options) => {
    setOptions(o);
    saveOptions(tool, resolveOptions(t.options, o));
  };

  const add = (files: File[]) => {
    addFiles(tool, resolveOptions(t.options, options), files);
    setIsHome(false);
  };

  // Full-Viewport Drag & Drop Listener
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
    setIsHome(false);
    setActivePresetNotice(null);
  };

  const selectHome = () => {
    location.hash = '#home';
    setIsHome(true);
    setActivePresetNotice(null);
  };

  const applyPreset = (preset: Preset) => {
    location.hash = `#${preset.tool}`;
    setTool(preset.tool);
    setIsHome(false);
    const defaultOpts = loadOptions(preset.tool);
    const merged: Options = preset.options ? { ...defaultOpts, ...preset.options } : defaultOpts;
    setOptions(merged);
    saveOptions(preset.tool, resolveOptions(TOOLS[preset.tool].options, merged));
    setActivePresetNotice(`Configured: ${preset.title}`);
  };

  const applyRecipe = (recipe: Recipe) => {
    location.hash = `#${recipe.tool}`;
    setTool(recipe.tool);
    setIsHome(false);
    const defaultOpts = loadOptions(recipe.tool);
    const merged: Options = recipe.options ? { ...defaultOpts, ...recipe.options } : defaultOpts;
    setOptions(merged);
    saveOptions(recipe.tool, resolveOptions(TOOLS[recipe.tool].options, merged));
    setActivePresetNotice(`Recipe activated: ${recipe.label}`);
  };

  const applyWorkflow = (targetTool: ToolId, customOptions?: Options) => {
    location.hash = `#${targetTool}`;
    setTool(targetTool);
    setIsHome(false);
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
    setIsHome(false);
    const targetOptions = loadOptions(targetTool);
    setOptions(targetOptions);
    addFiles(targetTool, resolveOptions(TOOLS[targetTool].options, targetOptions), newFiles);
  };

  // Telemetry metrics
  const activeJobsCount = jobs.filter((j) => j.state === 'Running' || j.state === 'Probing' || j.state === 'Verifying').length;
  const savedJobs = jobs.filter((j) => j.state === 'Saved');
  const completedWithSizes = savedJobs.filter((j) => j.inputSize && j.outputSize);
  const totalInputBytes = completedWithSizes.reduce((acc, j) => acc + (j.inputSize || 0), 0);

  const engineLabel =
    currentDomain === 'image'
      ? 'libwebp / mozjpeg / libpng WASM'
      : currentDomain === 'pdf'
      ? 'pdf-lib / qpdf WASM'
      : 'ffmpeg-wasm / PCM WebAudio';

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

      {/* Topbar: Wordmark, Status Pill, Search Trigger, Shortcuts, Theme */}
      <Topbar
        activeJobsCount={activeJobsCount}
        theme={theme}
        onToggleTheme={toggleTheme}
        onOpenSearch={() => setPaletteOpen(true)}
        onOpenShortcuts={() => setShortcutsOpen(true)}
        onSelectHome={selectHome}
      />

      {/* Semantic Accessible Coherent Navigation */}
      <Navigation
        currentTool={tool}
        isHome={isHome && jobs.length === 0}
        onSelectTool={selectTool}
        onSelectHome={selectHome}
      />

      {/* Main Content Workspace */}
      <main className="shell">
        {isHome && jobs.length === 0 ? (
          <HomePage
            onSelectTool={selectTool}
            onApplyPreset={applyPreset}
            onDropFiles={add}
            activePresetNotice={activePresetNotice}
            onDismissNotice={() => setActivePresetNotice(null)}
            defaultTool={tool}
          />
        ) : (
          <ToolWorkspacePage
            tool={tool}
            options={options}
            onOptionsChange={changeOptions}
            onResetOptions={() => changeOptions(resolveOptions(t.options))}
            jobs={jobs}
            onAddFiles={add}
            onCancelJob={cancel}
            onRetryJob={retry}
            onAnswerPassword={answerPassword}
            onUseInTool={useInTool}
            onRemoveJob={removeJob}
            onClearJobs={clearJobs}
            onReRunAll={() => reRunAll(tool, resolveOptions(t.options, options))}
            onSaveAllZip={async () => {
              const url = await saveAllUrl(jobs);
              setAllZipUrl(url);
              return url;
            }}
            allZipUrl={allZipUrl}
            onClearZipUrl={() => setAllZipUrl(undefined)}
            activePresetNotice={activePresetNotice}
            onDismissNotice={() => setActivePresetNotice(null)}
            onApplyRecipe={applyRecipe}
            onNavigateHome={selectHome}
          />
        )}

        {/* Status Toast Notice */}
        <div role="status" aria-live="polite" className="toast">
          {toast}
        </div>

        {/* Architectural Reference & FAQ below workspace */}
        <UseCasesSection
          onSelectTool={selectTool}
          onApplyWorkflow={applyWorkflow}
        />

        <FaqSection />
      </main>

      {/* Engine Telemetry Bar */}
      <TelemetryBar
        activeJobsCount={activeJobsCount}
        engineLabel={engineLabel}
        memoryEstimateMb={memoryEstimateMb}
        hasJobs={jobs.length > 0}
        onReRunQueue={() => reRunAll(tool, resolveOptions(t.options, options))}
      />

      {/* Footer */}
      <Footer
        onSelectTool={selectTool}
        onOpenShortcuts={() => setShortcutsOpen(true)}
        onOpenSearch={() => setPaletteOpen(true)}
      />

      {/* Mobile Bottom Navigation */}
      <MobileNavigation
        currentDomain={currentDomain}
        isHome={isHome && jobs.length === 0}
        onSelectTool={selectTool}
        onSelectHome={selectHome}
        onOpenSearch={() => setPaletteOpen(true)}
      />

      {/* Overlays */}
      <CommandPalette
        isOpen={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        onSelectTool={selectTool}
        onApplyPreset={applyPreset}
      />

      <ShortcutsModal
        isOpen={shortcutsOpen}
        onClose={() => setShortcutsOpen(false)}
      />
    </div>
  );
}
