import { useId, useState, type DragEvent } from 'react';
import { batchStatus, FORMATS, isTerminal, TOOLS, type Kind, type Options, type ToolId } from '@refract/core';
import { JobCard, size } from '../JobCard.tsx';
import { OptionsForm } from '../OptionsForm.tsx';
import type { JobView } from '../useJobs.ts';
import type { Preset } from '../config/presets.ts';
import { presetsForTool } from '../config/presets.ts';
import { domainOfTool } from '../config/domains.ts';
import { TOOLS_METADATA } from '../config/toolPresentation.ts';
import { DomainIcon } from '../components/layout/DomainIcon.tsx';
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

const KIND_HINTS: Record<Kind, string[]> = {
  image: ['HEIC', 'JPEG', 'PNG', 'WebP'],
  pdf: ['PDF', 'Password unlock', 'Multi-page'],
  audio: ['MP3', 'WAV', 'M4A', 'OGG'],
};

const KIND_PLURAL: Record<Kind, string> = {
  image: 'images',
  pdf: 'PDF documents',
  audio: 'audio files',
};

const BATCH_LABEL: Record<'processing' | 'waiting' | 'error' | 'finished', string> = {
  processing: 'Processing',
  waiting: 'Waiting',
  error: 'Needs attention',
  finished: 'Finished',
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
  onApplyPreset: (preset: Preset) => void;
  onSelectTool: (tool: ToolId) => void;
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
  onApplyPreset,
  onSelectTool,
  onNavigateHome,
}: ToolWorkspacePageProps) {
  const [isDropOver, setIsDropOver] = useState(false);
  const [sampleLoading, setSampleLoading] = useState(false);
  const inputId = useId();

  const t = TOOLS[tool];
  const meta = TOOLS_METADATA[tool];
  const domain = domainOfTool(tool);

  const savedJobs = jobs.filter((j) => j.state === 'Saved');
  const savedCount = savedJobs.length;
  const runningCount = jobs.filter((j) => j.state === 'Running' || j.state === 'Probing' || j.state === 'Verifying').length;
  const waitingCount = jobs.filter((j) => j.state === 'Queued' || j.state === 'NeedsPassword').length;
  const failedCount = jobs.filter((j) => j.state === 'Failed' || j.state === 'Rejected').length;
  const batch = jobs.length ? batchStatus(jobs.map((j) => j.state)) : null;
  const toolJobs = jobs.filter((j) => j.tool === tool);
  const quickPresets = presetsForTool(tool).filter((preset) => preset.options);

  const completedWithSizes = savedJobs.filter((j) => j.inputSize && j.outputSize);
  const totalInputBytes = completedWithSizes.reduce((acc, j) => acc + (j.inputSize || 0), 0);
  const totalOutputBytes = completedWithSizes.reduce((acc, j) => acc + (j.outputSize || 0), 0);
  const savedPercentage = totalInputBytes > 0 ? Math.round(((totalInputBytes - totalOutputBytes) / totalInputBytes) * 100) : 0;
  const sizeChange = totalOutputBytes - totalInputBytes;

  const handleDrop = (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDropOver(false);
    if (e.dataTransfer.files?.length) onAddFiles([...e.dataTransfer.files]);
  };

  const handleLoadSample = async () => {
    try {
      setSampleLoading(true);
      const count = t.minInputs && t.minInputs > 1 ? t.minInputs : 1;
      const sampleFiles: File[] = [];
      for (let i = 0; i < count; i++) {
        const suffix = count > 1 ? `-${i + 1}` : '';
        if (t.accepts === 'image') sampleFiles.push(await createSampleImage(suffix));
        else if (t.accepts === 'pdf') sampleFiles.push(await createSamplePdf(suffix));
        else sampleFiles.push(createSampleAudio(suffix));
      }
      onAddFiles(sampleFiles);
    } finally {
      setSampleLoading(false);
    }
  };

  return (
    <div className="workspace-page">
      <nav aria-label="Breadcrumb" className="workspace-breadcrumb">
        <a
          href="#home"
          onClick={(e) => {
            e.preventDefault();
            onNavigateHome();
          }}
        >
          All tools
        </a>
        <span aria-hidden="true">/</span>
        <button type="button" className="breadcrumb-domain" onClick={() => onSelectTool(domain.tools[0]!)}>
          {domain.shortLabel}
        </button>
        <span aria-hidden="true">/</span>
        <span aria-current="page">{t.label}</span>
      </nav>

      <header className="workspace-head">
        <div className="workspace-title-row">
          <span className="workspace-domain">
            <DomainIcon domain={domain.id} size={13} />
            {domain.label}
          </span>
          <h1 id="tool-title" className="workspace-title">
            {t.label}
          </h1>
          {meta?.engine && <span className="workspace-engine">{meta.engine}</span>}
        </div>
        <p className="workspace-desc">{meta?.description ?? t.blurb}</p>
        <div className="workspace-meta">
          <span className="meta-chip">Accepts {meta?.inputFormats.join(', ') ?? t.accepts}</span>
          <span className="meta-chip">{t.inputs === 'many' ? 'Files combine into one job' : 'One file per job'}</span>
          <span className="meta-chip meta-chip-quiet">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
              <path d="M7 11V7a5 5 0 0 1 10 0v4" />
            </svg>
            On-device
          </span>
        </div>
      </header>

      {activePresetNotice && (
        <div className="notice-bar" role="status">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <polyline points="20 6 9 17 4 12" />
          </svg>
          <span>{activePresetNotice}</span>
          {onDismissNotice && (
            <button type="button" className="notice-dismiss" onClick={onDismissNotice} aria-label="Dismiss notice">
              ×
            </button>
          )}
        </div>
      )}

      <div className="workspace-grid">
        <div className="workspace-main">
          <section className={`panel drop-panel${jobs.length ? ' is-compact' : ''}`} aria-labelledby="drop-heading">
            <label
              htmlFor={inputId}
              className={`dropzone workspace-dropzone${isDropOver ? ' over' : ''}`}
              data-testid="dropzone"
              onDragOver={(e) => {
                e.preventDefault();
                e.stopPropagation();
                setIsDropOver(true);
              }}
              onDragLeave={() => setIsDropOver(false)}
              onDrop={handleDrop}
            >
              <span className="dropzone-inner">
                <span className="dropzone-icon" aria-hidden="true">
                  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                    <polyline points="17 8 12 3 7 8" />
                    <line x1="12" y1="3" x2="12" y2="15" />
                  </svg>
                </span>
                <span className="dropzone-copy">
                  <strong id="drop-heading" className="dropzone-title">
                    {jobs.length ? `Add more ${KIND_PLURAL[t.accepts]}` : `Drop ${KIND_PLURAL[t.accepts]} here`}
                  </strong>
                  <span className="dropzone-sub">
                    or click to browse · paste with <kbd>Ctrl V</kbd>
                  </span>
                </span>
                {!jobs.length && (
                  <span className="dropzone-formats">
                    {KIND_HINTS[t.accepts].map((hint) => (
                      <span key={hint} className="format-pill">
                        {hint}
                      </span>
                    ))}
                  </span>
                )}
                <span className="dropzone-sample">
                  <button
                    type="button"
                    className="link"
                    disabled={sampleLoading}
                    onClick={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      void handleLoadSample();
                    }}
                  >
                    {sampleLoading ? 'Preparing sample…' : 'Try a sample file'}
                  </button>
                </span>
              </span>
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

          {jobs.length > 0 && (
            <section className="panel queue-panel" aria-labelledby="queue-heading">
              <div className="queue-head">
                <div className="queue-head-main">
                  <h2 id="queue-heading" className="queue-title">
                    Queue
                  </h2>
                  <span className="queue-count">{jobs.length}</span>
                  {batch && (
                    <span className={`queue-batch is-${batch}`} data-batch={batch}>
                      <span className="queue-batch-dot" aria-hidden="true" />
                      {BATCH_LABEL[batch]}
                    </span>
                  )}
                </div>

                <div className="queue-head-side">
                  {runningCount > 0 && <span className="status-chip is-running">{runningCount} active</span>}
                  {waitingCount > 0 && <span className="status-chip is-waiting">{waitingCount} waiting</span>}
                  {savedCount > 0 && <span className="status-chip is-saved">{savedCount} ready</span>}
                  {failedCount > 0 && <span className="status-chip is-failed">{failedCount} failed</span>}
                  {totalInputBytes > 0 && totalOutputBytes > 0 && savedPercentage !== 0 && (
                    <span
                      className={`queue-savings${sizeChange > 0 ? ' is-grown' : ''}`}
                      title={sizeChange > 0 ? 'Total size increase across finished jobs' : 'Total size saved across finished jobs'}
                    >
                      {savedPercentage > 0 ? `−${savedPercentage}%` : `+${Math.abs(savedPercentage)}%`}
                      <span>{size(Math.abs(sizeChange))}</span>
                    </span>
                  )}
                  {jobs.some((j) => isTerminal(j.state)) && (
                    <button type="button" className="link" onClick={onClearJobs}>
                      Clear finished
                    </button>
                  )}
                </div>
              </div>

              <ul className="jobs" aria-label="Jobs">
                {jobs.map((job) => (
                  <JobCard
                    key={job.id}
                    job={job}
                    onCancel={() => onCancelJob(job.id)}
                    onRetry={() => onRetryJob(job.id)}
                    onPassword={(pw, all) => onAnswerPassword(job.id, pw, all)}
                    onUseInTool={onUseInTool}
                    onRemove={() => onRemoveJob(job.id)}
                  />
                ))}
              </ul>
            </section>
          )}
        </div>

        <aside className="workspace-side">
          <div className="side-sticky">
            <section className="panel settings-panel" aria-labelledby="settings-heading">
              <div className="panel-head">
                <h2 id="settings-heading">Settings</h2>
                <span className="panel-tag">{t.label}</span>
              </div>

              {t.options.length > 0 ? (
                <OptionsForm specs={t.options} values={options} onChange={onOptionsChange} onReset={onResetOptions} />
              ) : (
                <p className="panel-empty">This tool has no settings. Drop a file and it runs with sensible defaults.</p>
              )}

              {quickPresets.length > 0 && (
                <div className="quick-presets">
                  <span className="quick-presets-label">Quick presets</span>
                  <div className="quick-preset-row">
                    {quickPresets.map((preset) => (
                      <button key={preset.id} type="button" className="quick-preset" onClick={() => onApplyPreset(preset)}>
                        {preset.title}
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </section>

            <section className="panel export-panel" aria-labelledby="export-heading">
              <div className="panel-head">
                <h2 id="export-heading">Export</h2>
                <span className="panel-tag">{savedCount === 1 ? '1 file ready' : `${savedCount} files ready`}</span>
              </div>

              {savedCount > 1 ? (
                allZipUrl ? (
                  <a
                    className="button primary export-button"
                    href={allZipUrl}
                    download="refract.zip"
                    onClick={() => setTimeout(onClearZipUrl, 0)}
                  >
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                      <polyline points="7 10 12 15 17 10" />
                      <line x1="12" y1="15" x2="12" y2="3" />
                    </svg>
                    Download refract.zip
                  </a>
                ) : (
                  <button type="button" className="button primary export-button" onClick={onSaveAllZip}>
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                      <polyline points="7 10 12 15 17 10" />
                      <line x1="12" y1="15" x2="12" y2="3" />
                    </svg>
                    Save all as ZIP
                  </button>
                )
              ) : savedCount === 1 && savedJobs[0]?.url ? (
                <a
                  className="button primary export-button"
                  href={savedJobs[0].url}
                  download={savedJobs[0].outputName}
                >
                  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                    <polyline points="7 10 12 15 17 10" />
                    <line x1="12" y1="15" x2="12" y2="3" />
                  </svg>
                  Download {savedJobs[0].outputName}
                </a>
              ) : (
                <p className="export-empty">
                  {runningCount > 0
                    ? 'Working in your browser. Finished files appear here.'
                    : 'Finished files appear here, ready to download.'}
                </p>
              )}

              {toolJobs.length > 0 && (
                <button type="button" className="button export-rerun" onClick={onReRunAll}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
                    <path d="M3 3v5h5" />
                  </svg>
                  Re-process
                </button>
              )}
            </section>
          </div>
        </aside>
      </div>
    </div>
  );
}
