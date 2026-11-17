import { useId, useState, type DragEvent } from 'react';
import { batchStatus, FORMATS, resolveOptions, TOOLS, type Kind, type Options, type ToolId } from '@refract/core';
import { JobCard, size } from '../JobCard.tsx';
import { OptionsForm } from '../OptionsForm.tsx';
import type { JobView } from '../useJobs.ts';
import { RECIPES_BY_DOMAIN, type DomainId, type Recipe } from '../config/recipes.ts';
import { TOOLS_METADATA } from '../config/toolPresentation.ts';
import { createSampleAudio, createSampleImage, createSamplePdf } from '../sampleFiles.ts';

const ACCEPT: Record<Kind, string> = Object.fromEntries(
  (['image', 'pdf', 'audio'] as Kind[]).map((k) => [
    k,
    Object.values(FORMATS)
      .filter((f) => f.kind === k)
      .flatMap((f) => [...f.ext.map((e) => `.${e}`), f.mime])
      .join(','),
  ]),
) as Record<Kind, string>;

const FORMAT_HINTS: Record<Kind, string[]> = {
  image: ['HEIC', 'JPEG', 'PNG', 'WebP'],
  pdf: ['PDF', 'Password unlock', 'Multi-page'],
  audio: ['MP3', 'WAV', 'M4A', 'OGG'],
};

interface ToolWorkspacePageProps {
  tool: ToolId;
  options: Options;
  onOptionsChange: (o: Options) => void;
  onResetOptions: () => void;
  jobs: JobView[];
  onAddFiles: (files: File[]) => void;
  onCancelJob: (id: string) => void;
  onRetryJob: (id: string) => void;
  onAnswerPassword: (id: string, pw: string | null, forAll: boolean) => void;
  onUseInTool: (targetTool: ToolId, job: JobView) => void;
  onRemoveJob: (id: string) => void;
  onClearJobs: () => void;
  onReRunAll: () => void;
  onSaveAllZip: () => Promise<string>;
  allZipUrl?: string;
  onClearZipUrl: () => void;
  activePresetNotice?: string | null;
  onDismissNotice?: () => void;
  onApplyRecipe: (recipe: Recipe) => void;
  onNavigateHome: () => void;
}

export function ToolWorkspacePage({
  tool,
  options,
  onOptionsChange,
  onResetOptions,
  jobs,
  onAddFiles,
  onCancelJob,
  onRetryJob,
  onAnswerPassword,
  onUseInTool,
  onRemoveJob,
  onClearJobs,
  onReRunAll,
  onSaveAllZip,
  allZipUrl,
  onClearZipUrl,
  activePresetNotice,
  onDismissNotice,
  onApplyRecipe,
  onNavigateHome,
}: ToolWorkspacePageProps) {
  const [isDropOver, setIsDropOver] = useState(false);
  const [sampleLoading, setSampleLoading] = useState(false);
  const inputId = useId();

  const t = TOOLS[tool];
  const meta = TOOLS_METADATA[tool];
  const domainLabel = t.accepts === 'image' ? 'Images' : t.accepts === 'pdf' ? 'PDF' : 'Audio';
  const domainId = (t.accepts === 'image' ? 'image' : t.accepts === 'pdf' ? 'pdf' : 'audio') as DomainId;

  // Filter jobs relevant to current context or all
  const savedJobs = jobs.filter((j) => j.state === 'Saved');
  const savedCount = savedJobs.length;
  const runningCount = jobs.filter((j) => j.state === 'Running' || j.state === 'Probing' || j.state === 'Verifying').length;
  const waitingCount = jobs.filter((j) => j.state === 'Queued').length;
  const failedCount = jobs.filter((j) => j.state === 'Failed' || j.state === 'Rejected').length;
  const batch = jobs.length ? batchStatus(jobs.map((j) => j.state)) : null;

  const completedWithSizes = savedJobs.filter((j) => j.inputSize && j.outputSize);
  const totalInputBytes = completedWithSizes.reduce((acc, j) => acc + (j.inputSize || 0), 0);
  const totalOutputBytes = completedWithSizes.reduce((acc, j) => acc + (j.outputSize || 0), 0);
  const savedPercentage = totalInputBytes > 0 ? Math.round(((totalInputBytes - totalOutputBytes) / totalInputBytes) * 100) : 0;
  const totalSavedBytes = Math.max(0, totalInputBytes - totalOutputBytes);

  const handleDrop = (e: DragEvent) => {
    e.preventDefault();
    setIsDropOver(false);
    if (e.dataTransfer.files?.length) {
      onAddFiles([...e.dataTransfer.files]);
    }
  };

  const handleLoadSample = async () => {
    try {
      setSampleLoading(true);
      let file: File;
      if (t.accepts === 'image') {
        file = await createSampleImage();
      } else if (t.accepts === 'pdf') {
        file = await createSamplePdf();
      } else {
        file = createSampleAudio();
      }
      onAddFiles([file]);
    } finally {
      setSampleLoading(false);
    }
  };

  return (
    <div className="tool-workspace-page">
      {/* 1. Breadcrumbs */}
      <nav aria-label="Breadcrumb" className="workspace-breadcrumb">
        <ol className="breadcrumb-list">
          <li className="breadcrumb-item">
            <a
              href="#home"
              onClick={(e) => {
                e.preventDefault();
                onNavigateHome();
              }}
            >
              Workspace
            </a>
          </li>
          <li className="breadcrumb-separator" aria-hidden="true">/</li>
          <li className="breadcrumb-item">
            <span>{domainLabel}</span>
          </li>
          <li className="breadcrumb-separator" aria-hidden="true">/</li>
          <li className="breadcrumb-item is-active" aria-current="page">
            <span>{t.label}</span>
          </li>
        </ol>
      </nav>

      {/* 2. Tool Heading */}
      <header className="workspace-tool-header">
        <div className="tool-title-row">
          <h1 id="tool-title" className="workspace-tool-title">
            {t.label}
          </h1>
          <span className="tool-engine-badge">{meta?.engine ?? 'WASM Engine'}</span>
        </div>
        <p className="workspace-tool-description">
          {meta?.description || t.blurb}
        </p>
        <div className="workspace-tool-meta-bar">
          <div className="meta-bar-item">
            <span className="meta-bar-label">Input formats:</span>
            <span className="meta-bar-value">{meta?.inputFormats.join(', ') ?? t.accepts}</span>
          </div>
          <div className="meta-bar-item">
            <span className="meta-bar-label">Output:</span>
            <span className="meta-bar-value">{meta?.outputCapability ?? 'Processed file'}</span>
          </div>
          <div className="meta-bar-item">
            <span className="meta-bar-label">Processing:</span>
            <span className="meta-bar-value">100% On-device WASM</span>
          </div>
        </div>
      </header>

      {/* Active Preset Notification Banner */}
      {activePresetNotice && (
        <div className="preset-notice-bar" role="status">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="notice-icon">
            <polyline points="20 6 9 17 4 12" />
          </svg>
          <span>{activePresetNotice}</span>
          {onDismissNotice && (
            <button
              type="button"
              className="notice-dismiss"
              onClick={onDismissNotice}
              aria-label="Dismiss notice"
            >
              ×
            </button>
          )}
        </div>
      )}

      {/* 3. File Staging Surface (Hero when empty, Compact when jobs exist) */}
      {jobs.length === 0 ? (
        <div className="workspace-empty-layout">
          {/* 01 / File Staging Dropzone */}
          <section className="stage-section" aria-labelledby="stage-heading">
            <div className="section-label-row">
              <span className="section-step-num">01</span>
              <span className="section-step-slash">/</span>
              <h2 id="stage-heading" className="section-step-title">Stage files</h2>
              <span className="section-target-badge">{t.label}</span>
            </div>

            <label
              htmlFor={inputId}
              className={`dropzone utility-stage-dropzone${isDropOver ? ' over' : ''}`}
              data-testid="dropzone"
              onDragOver={(e) => {
                e.preventDefault();
                setIsDropOver(true);
              }}
              onDragLeave={() => setIsDropOver(false)}
              onDrop={handleDrop}
            >
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
                  Drop {t.accepts === 'image' ? 'images' : t.accepts === 'pdf' ? 'PDF documents' : 'audio tracks'} to convert
                </strong>
                <span className="stage-drop-subtitle">
                  Click to browse  ·  Paste from clipboard ⌘V
                </span>

                <div className="stage-format-pills">
                  {FORMAT_HINTS[t.accepts]?.map((fmt) => (
                    <span key={fmt} className="stage-format-chip">
                      {fmt}
                    </span>
                  ))}
                </div>

                {/* Sample Generator Button */}
                <div className="stage-quick-start" onClick={(e) => e.stopPropagation()}>
                  <span className="stage-quick-label">Try sample:</span>
                  <button
                    type="button"
                    className="stage-sample-btn"
                    disabled={sampleLoading}
                    onClick={handleLoadSample}
                  >
                    {sampleLoading ? 'Generating…' : `Sample ${domainLabel} file`}
                  </button>
                </div>
              </div>

              <input
                id={inputId}
                className="visually-hidden"
                type="file"
                multiple
                accept={ACCEPT[t.accepts]}
                onChange={(e) => {
                  if (e.target.files?.length) {
                    onAddFiles([...e.target.files]);
                    e.target.value = '';
                  }
                }}
              />
            </label>
          </section>

          {/* 02 / Target Pipeline Configuration */}
          <section className="pipeline-section" aria-labelledby="pipeline-heading">
            <div className="section-label-row">
              <span className="section-step-num">02</span>
              <span className="section-step-slash">/</span>
              <h2 id="pipeline-heading" className="section-step-title">
                Configuration ({t.label})
              </h2>
              <span className="pipeline-tool-desc">{t.blurb}</span>
            </div>

            <div className="pipeline-dock">
              <OptionsForm
                specs={t.options}
                values={options}
                onChange={onOptionsChange}
                onReset={onResetOptions}
              />
            </div>
          </section>

          {/* 03 / Quick Recipes */}
          <section className="recipes-section" aria-labelledby="recipes-heading">
            <div className="section-label-row">
              <span className="section-step-num">03</span>
              <span className="section-step-slash">/</span>
              <h2 id="recipes-heading" className="section-step-title">Quick recipes</h2>
              <span className="recipes-domain-tag">({domainLabel})</span>
            </div>

            <div className="recipes-grid">
              {RECIPES_BY_DOMAIN[domainId]?.map((r) => (
                <button
                  key={r.id}
                  type="button"
                  className="recipe-card-btn"
                  onClick={() => onApplyRecipe(r)}
                >
                  <span className="recipe-btn-title">{r.label}</span>
                  <span className="recipe-btn-arrow">→</span>
                </button>
              ))}
            </div>
          </section>
        </div>
      ) : (
        /* Workspace Active Processing Layout (Queue Left + Output Settings Right) */
        <div className="workspace-active-layout">
          {/* Top Compact File Staging Bar */}
          <div className="queue-top-action-bar">
            <label
              htmlFor={inputId}
              className={`queue-top-dropbar${isDropOver ? ' over' : ''}`}
              data-testid="dropzone"
              onDragOver={(e) => {
                e.preventDefault();
                setIsDropOver(true);
              }}
              onDragLeave={() => setIsDropOver(false)}
              onDrop={handleDrop}
            >
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <line x1="12" y1="5" x2="12" y2="19" />
                <line x1="5" y1="12" x2="19" y2="12" />
              </svg>
              <span className="queue-dropbar-text">
                Add more files to queue or drop anywhere (⌘V)
              </span>
              <span className="queue-dropbar-badge">{t.accepts}</span>

              <input
                id={inputId}
                className="visually-hidden"
                type="file"
                multiple
                accept={ACCEPT[t.accepts]}
                onChange={(e) => {
                  if (e.target.files?.length) {
                    onAddFiles([...e.target.files]);
                    e.target.value = '';
                  }
                }}
              />
            </label>

            <div className="queue-top-actions">
              {savedCount > 0 && (
                <button
                  type="button"
                  className="button clear-btn queue-clear-action"
                  onClick={onClearJobs}
                  title="Clear finished jobs from queue"
                >
                  Clear Finished (✕)
                </button>
              )}
            </div>
          </div>

          {/* Split Queue + Persistent Output Inspector */}
          <div className="queue-split-layout">
            {/* Left Column: Processing Queue & Job Cards */}
            <div className="queue-main-col">
              <div className="queue-header-row">
                <div className="queue-header-left">
                  <span className="queue-header-label">Processing Queue</span>
                  <span className="queue-count-pill">
                    {jobs.length} {jobs.length === 1 ? 'file' : 'files'}
                  </span>
                  {batch && (
                    <span className="queue-batch-status" data-batch={batch}>
                      ● {batch}
                    </span>
                  )}
                  <div className="queue-status-breakdown">
                    {runningCount > 0 && <span className="status-chip running">{runningCount} active</span>}
                    {waitingCount > 0 && <span className="status-chip waiting">{waitingCount} waiting</span>}
                    {savedCount > 0 && <span className="status-chip saved">{savedCount} ready</span>}
                    {failedCount > 0 && <span className="status-chip failed">{failedCount} failed</span>}
                  </div>
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
                    onCancel={() => onCancelJob(j.id)}
                    onRetry={() => onRetryJob(j.id)}
                    onPassword={(pw, all) => onAnswerPassword(j.id, pw, all)}
                    onUseInTool={onUseInTool}
                    onRemove={() => onRemoveJob(j.id)}
                  />
                ))}
              </ul>
            </div>

            {/* Right Column: Output Inspector & Batch Settings */}
            <div className="queue-inspector-col">
              <div className="inspector-dock-sticky">
                <div className="inspector-header">
                  <div className="section-label-row">
                    <span className="section-step-num">02</span>
                    <span className="section-step-slash">/</span>
                    <h3 className="section-step-title">Output configuration</h3>
                  </div>
                  <span className="inspector-tool-tag">{t.label}</span>
                </div>

                {/* Data-Driven Settings Form */}
                <div className="inspector-options-body">
                  <OptionsForm
                    specs={t.options}
                    values={options}
                    onChange={onOptionsChange}
                    onReset={onResetOptions}
                  />
                </div>

                {/* Batch Action Box */}
                <div className="inspector-download-box">
                  {savedCount > 1 ? (
                    allZipUrl ? (
                      <a
                        className="button primary inspector-zip-btn"
                        href={allZipUrl}
                        download="refract.zip"
                        onClick={() => setTimeout(onClearZipUrl, 0)}
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
                        onClick={onSaveAllZip}
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
                      <span className="waiting-spinner" aria-hidden="true" />
                      <span>
                        {runningCount > 0 ? 'Processing files in WebAssembly memory…' : 'Queue ready for new files'}
                      </span>
                    </div>
                  )}

                  {/* Re-process entire queue button */}
                  <button
                    type="button"
                    className="button subtle-btn rerun-all-btn"
                    onClick={onReRunAll}
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
    </div>
  );
}
