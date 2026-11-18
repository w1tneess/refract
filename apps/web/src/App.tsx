import { useEffect, useRef, useState } from 'react';
import { FORMATS, resolveOptions, TOOLS, type Options, type ToolId } from '@refract/core';
import { useJobs, type JobView } from './useJobs.ts';
import { useTheme } from './hooks/useTheme.ts';
import { Topbar } from './components/layout/Topbar.tsx';
import { Sidebar } from './components/layout/Sidebar.tsx';
import { MobileNavigation } from './components/layout/MobileNavigation.tsx';
import { Footer } from './components/layout/Footer.tsx';
import { HomePage } from './pages/HomePage.tsx';
import { ToolWorkspacePage } from './pages/ToolWorkspacePage.tsx';
import { CommandPalette } from './CommandPalette.tsx';
import { ShortcutsModal } from './ShortcutsModal.tsx';
import { DOMAINS, domainIdOfTool } from './config/domains.ts';
import { describeGroups, groupFilesByTool } from './config/routing.ts';
import type { Preset } from './config/presets.ts';

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
  const [isHome, setIsHome] = useState<boolean>(isHomeFromHash);
  const [options, setOptions] = useState<Options>(() => loadOptions(toolFromHash()));
  const [allZipUrl, setAllZipUrl] = useState<string>();
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [activePresetNotice, setActivePresetNotice] = useState<string | null>(null);
  const [isDraggingViewport, setIsDraggingViewport] = useState(false);
  const dragCounter = useRef(0);
  const skipNextHash = useRef<string | null>(null);
  const pendingZipRevoke = useRef<number | null>(null);

  const t = TOOLS[tool];
  const runningCount = jobs.filter((j) => j.state === 'Running' || j.state === 'Probing' || j.state === 'Verifying').length;

  useEffect(() => {
    document.title = isHome ? 'Refract — Local-First File Converter' : `${TOOLS[tool].label} — Refract`;
  }, [isHome, tool]);

  const setHashProgrammatically = (hash: string) => {
    if (location.hash !== hash) {
      skipNextHash.current = hash;
      location.hash = hash;
    }
  };

  // Sync hash changes (back/forward buttons, hand-edited URLs, links in job cards).
  useEffect(() => {
    const onHash = () => {
      if (skipNextHash.current === location.hash) {
        skipNextHash.current = null;
        return;
      }
      skipNextHash.current = null;
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

  // Revoke any active allZipUrl on unmount.
  useEffect(() => {
    return () => {
      if (pendingZipRevoke.current !== null) clearTimeout(pendingZipRevoke.current);
      if (allZipUrl) URL.revokeObjectURL(allZipUrl);
    };
  }, [allZipUrl]);

  const changeOptions = (o: Options) => {
    setOptions(o);
    saveOptions(tool, resolveOptions(t.options, o));
  };

  /** Add files to the tool that is already open. */
  const addHere = (files: File[]) => {
    if (!files.length) return;
    addFiles(tool, resolveOptions(t.options, options), files);
    setIsHome(false);
  };

  /**
   * Add files without an explicit destination. Files go to the current tool when
   * it understands them, and to the obvious default tool otherwise. Mixed
   * batches are split, so dropping photos and a PDF together still works.
   */
  const addAnywhere = (files: File[]) => {
    if (!files.length) return;
    const groups = groupFilesByTool(files, tool);

    if (!groups.length) {
      addHere(files);
      return;
    }

    for (const group of groups) {
      const groupOptions = group.tool === tool ? resolveOptions(t.options, options) : loadOptions(group.tool);
      addFiles(group.tool, groupOptions, group.files);
    }

    const first = groups[0]!;
    if (first.tool !== tool) {
      const nextOptions = loadOptions(first.tool);
      setTool(first.tool);
      setOptions(nextOptions);
      setHashProgrammatically(`#${first.tool}`);
    }
    if (groups.length > 1) {
      setActivePresetNotice(`Routed ${files.length} files: ${describeGroups(groups)}`);
    }
    setIsHome(false);
  };

  // Full-viewport drag feedback. Drops outside a dropzone are handled here.
  useEffect(() => {
    const onDragEnter = (e: globalThis.DragEvent) => {
      e.preventDefault();
      dragCounter.current += 1;
      if (e.dataTransfer?.types?.includes('Files')) setIsDraggingViewport(true);
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
      const target = e.target as HTMLElement | null;
      if (target?.closest?.('[data-testid="dropzone"]')) return;
      const files = [...(e.dataTransfer?.files ?? [])];
      if (!files.length) return;
      if (isHome) addAnywhere(files);
      else addHere(files);
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isHome, tool, options]);

  // Clipboard paste and global shortcuts.
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT')) return;
      const files = Array.from(e.clipboardData?.files ?? []);
      if (!files.length) return;
      e.preventDefault();
      if (isHome) addAnywhere(files);
      else addHere(files);
    };

    const onKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      const isTyping = Boolean(
        target &&
          (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT' || target.isContentEditable),
      );
      const hasModifier = e.metaKey || e.ctrlKey || e.altKey;

      if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setPaletteOpen((prev) => !prev);
      } else if (!isTyping && !hasModifier && !paletteOpen && !shortcutsOpen && e.key === '?') {
        e.preventDefault();
        setShortcutsOpen(true);
      } else if (!isTyping && !hasModifier && !paletteOpen && !shortcutsOpen && e.key === '1') {
        selectTool(DOMAINS[0]!.tools[0]!);
      } else if (!isTyping && !hasModifier && !paletteOpen && !shortcutsOpen && e.key === '2') {
        selectTool(DOMAINS[1]!.tools[0]!);
      } else if (!isTyping && !hasModifier && !paletteOpen && !shortcutsOpen && e.key === '3') {
        selectTool(DOMAINS[2]!.tools[0]!);
      }
    };

    window.addEventListener('paste', onPaste);
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('paste', onPaste);
      window.removeEventListener('keydown', onKeyDown);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isHome, tool, options, paletteOpen, shortcutsOpen]);

  const selectTool = (targetTool: ToolId) => {
    setHashProgrammatically(`#${targetTool}`);
    setTool(targetTool);
    setOptions(loadOptions(targetTool));
    setIsHome(false);
    setActivePresetNotice(null);
  };

  const selectHome = () => {
    setHashProgrammatically('#home');
    setIsHome(true);
    setActivePresetNotice(null);
  };

  const applyPreset = (preset: Preset) => {
    setHashProgrammatically(`#${preset.tool}`);
    setTool(preset.tool);
    setIsHome(false);
    const defaultOpts = loadOptions(preset.tool);
    const merged: Options = preset.options ? { ...defaultOpts, ...preset.options } : defaultOpts;
    setOptions(merged);
    saveOptions(preset.tool, resolveOptions(TOOLS[preset.tool].options, merged));
    setActivePresetNotice(`Applied “${preset.title}”. Drop a file to run it.`);
  };

  const useInTool = (targetTool: ToolId, job: JobView) => {
    if (!job.outputs?.length) return;
    const newFiles = job.outputs.map((o) => new File([o.bytes as BlobPart], o.name, { type: FORMATS[o.format].mime }));
    setHashProgrammatically(`#${targetTool}`);
    setTool(targetTool);
    setIsHome(false);
    const targetOptions = loadOptions(targetTool);
    setOptions(targetOptions);
    setActivePresetNotice(null);
    addFiles(targetTool, resolveOptions(TOOLS[targetTool].options, targetOptions), newFiles);
  };

  return (
    <div className="app-shell">
      <a href="#main-content" className="skip-link">
        Skip to main content
      </a>

      {isDraggingViewport && (
        <div className="drag-overlay" aria-hidden="true">
          <div className="drag-overlay-box">
            <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
              <polyline points="17 8 12 3 7 8" />
              <line x1="12" y1="3" x2="12" y2="15" />
            </svg>
            <strong>Drop files to add them</strong>
            <span>Everything is processed on this device.</span>
          </div>
        </div>
      )}

      <Sidebar
        currentTool={tool}
        isHome={isHome}
        jobCount={jobs.length}
        onSelectTool={selectTool}
        onSelectHome={selectHome}
      />

      <div className="app-body">
        <Topbar
          tool={tool}
          isHome={isHome}
          jobCount={jobs.length}
          runningCount={runningCount}
          theme={theme}
          onToggleTheme={toggleTheme}
          onOpenSearch={() => setPaletteOpen(true)}
          onOpenShortcuts={() => setShortcutsOpen(true)}
          onSelectHome={selectHome}
          onSelectTool={selectTool}
        />

        <main id="main-content" className="shell" tabIndex={-1}>
          {isHome ? (
            <HomePage
              jobCount={jobs.length}
              onOpenQueue={() => selectTool(jobs[jobs.length - 1]?.tool ?? tool)}
              onSelectTool={selectTool}
              onApplyPreset={applyPreset}
              onDropFiles={addAnywhere}
              activePresetNotice={activePresetNotice}
              onDismissNotice={() => setActivePresetNotice(null)}
            />
          ) : (
            <ToolWorkspacePage
              tool={tool}
              options={options}
              onOptionsChange={changeOptions}
              onResetOptions={() => changeOptions(resolveOptions(t.options))}
              jobs={jobs}
              onAddFiles={addHere}
              onCancelJob={cancel}
              onRetryJob={retry}
              onAnswerPassword={answerPassword}
              onUseInTool={useInTool}
              onRemoveJob={removeJob}
              onClearJobs={clearJobs}
              onReRunAll={() => reRunAll(tool, resolveOptions(t.options, options))}
              onSaveAllZip={async () => {
                if (pendingZipRevoke.current !== null) {
                  clearTimeout(pendingZipRevoke.current);
                  pendingZipRevoke.current = null;
                }
                const url = await saveAllUrl(jobs);
                setAllZipUrl(url);
                return url;
              }}
              allZipUrl={allZipUrl}
              onClearZipUrl={() => {
                const urlToRevoke = allZipUrl;
                setAllZipUrl(undefined);
                if (urlToRevoke) {
                  pendingZipRevoke.current = window.setTimeout(() => {
                    URL.revokeObjectURL(urlToRevoke);
                    pendingZipRevoke.current = null;
                  }, 30_000);
                }
              }}
              activePresetNotice={activePresetNotice}
              onDismissNotice={() => setActivePresetNotice(null)}
              onApplyPreset={applyPreset}
              onSelectTool={selectTool}
              onNavigateHome={selectHome}
            />
          )}

          <div role="status" aria-live="polite" aria-atomic="true" className="toast">
            {toast}
          </div>
        </main>

        <Footer onOpenShortcuts={() => setShortcutsOpen(true)} onOpenSearch={() => setPaletteOpen(true)} />
      </div>

      <MobileNavigation
        currentDomain={domainIdOfTool(tool)}
        isHome={isHome}
        onSelectTool={selectTool}
        onSelectHome={selectHome}
        onOpenSearch={() => setPaletteOpen(true)}
      />

      <CommandPalette
        isOpen={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        onSelectTool={selectTool}
        onApplyPreset={applyPreset}
      />

      <ShortcutsModal isOpen={shortcutsOpen} onClose={() => setShortcutsOpen(false)} />
    </div>
  );
}
