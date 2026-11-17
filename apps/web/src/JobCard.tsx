import { useId, useState } from 'react';
import { describeError, FORMATS, isTerminal, TOOLS, type ToolId } from '@refract/core';
import type { JobView } from './useJobs.ts';

export const size = (n?: number) =>
  n === undefined ? '—' : n < 1024 * 1024 ? `${Math.max(1, Math.round(n / 1024))} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`;

const STATE_LABEL: Record<JobView['state'], string> = {
  Queued: 'Waiting',
  Probing: 'Checking file',
  NeedsPassword: 'Needs password',
  Running: 'Working',
  Verifying: 'Checking result',
  Saved: 'Done',
  Rejected: "Can't do this",
  Failed: 'Failed',
  Canceled: 'Canceled',
};

function KindIcon({ tool }: { tool: ToolId }) {
  const accepts = TOOLS[tool]?.accepts ?? 'image';
  if (accepts === 'pdf') {
    return (
      <svg className="job-kind-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
        <polyline points="14 2 14 8 20 8" />
        <line x1="16" y1="13" x2="8" y2="13" />
        <line x1="16" y1="17" x2="8" y2="17" />
      </svg>
    );
  }
  if (accepts === 'audio') {
    return (
      <svg className="job-kind-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M9 18V5l12-2v13" />
        <circle cx="6" cy="18" r="3" />
        <circle cx="18" cy="16" r="3" />
      </svg>
    );
  }
  return (
    <svg className="job-kind-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
      <circle cx="8.5" cy="8.5" r="1.5" />
      <polyline points="21 15 16 10 5 21" />
    </svg>
  );
}

function PasswordForm({ job, onAnswer }: { job: JobView; onAnswer: (pw: string | null, forAll: boolean) => void }) {
  const id = useId();
  const [pw, setPw] = useState('');
  const [all, setAll] = useState(false);
  const [show, setShow] = useState(false);

  return (
    <form className="password" onSubmit={(e) => { e.preventDefault(); onAnswer(pw, all); }}>
      <label htmlFor={`${id}-pw`} className="password-label">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
          <path d="M7 11V7a5 5 0 0 1 10 0v4" />
        </svg>
        <span>
          <strong>{job.password!.file}</strong> is password-protected
          {job.password!.wrong ? '. That password is wrong; try again' : ''}.
        </span>
      </label>
      <div className="password-input-row">
        <input
          id={`${id}-pw`}
          type={show ? 'text' : 'password'}
          autoComplete="current-password"
          value={pw}
          placeholder="Password"
          onChange={(e) => setPw(e.target.value)}
          aria-invalid={job.password!.wrong || undefined}
        />
        <button
          type="button"
          className="button subtle-btn"
          onClick={() => setShow(!show)}
          aria-label={show ? 'Hide password' : 'Show password'}
        >
          {show ? 'Hide' : 'Show'}
        </button>
      </div>
      <label className="check">
        <input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} />
        <span>Use for all locked files</span>
      </label>
      <div className="job-actions">
        <button type="submit" className="primary">Unlock</button>
        <button type="button" onClick={() => onAnswer(null, all)}>Skip this file</button>
      </div>
    </form>
  );
}

export function JobCard({
  job,
  onCancel,
  onRetry,
  onPassword,
  onUseInTool,
  onRemove,
}: {
  job: JobView;
  onCancel: () => void;
  onRetry: () => void;
  onPassword: (pw: string | null, forAll: boolean) => void;
  onUseInTool?: (targetTool: ToolId, job: JobView) => void;
  onRemove?: () => void;
}) {
  const isBusy = !isTerminal(job.state);
  const title = job.inputNames.length > 1 ? `${job.inputName} + ${job.inputNames.length - 1} more` : job.inputName;
  const outKind = job.outputs?.[0] ? FORMATS[job.outputs[0]!.format].kind : null;
  const compatibleTools = outKind ? (Object.keys(TOOLS) as ToolId[]).filter((id) => TOOLS[id].accepts === outKind && id !== job.tool) : [];

  // Calculate size change percentage
  let sizeDeltaBadge = null;
  if (job.inputSize && job.outputSize && job.inputSize > 0) {
    const pct = Math.round(((job.outputSize - job.inputSize) / job.inputSize) * 100);
    if (pct < 0) {
      sizeDeltaBadge = <span className="job-delta job-delta-saved">{pct}%</span>;
    } else if (pct > 0) {
      sizeDeltaBadge = <span className="job-delta job-delta-increased">+{pct}%</span>;
    }
  }

  return (
    <li className={`job job-${job.state.toLowerCase()}`} data-state={job.state} data-timings={JSON.stringify(job.timings)}>
      <div className="job-head">
        <div className="job-title-group">
          <KindIcon tool={job.tool} />
          <span className="job-name" title={job.inputNames.join('\n')}>
            {title}
          </span>
        </div>
        <div className="job-head-meta">
          <span className="job-state" role="status">
            <span className="state-dot" aria-hidden="true" />
            {STATE_LABEL[job.state]}
          </span>
          {onRemove && !isBusy && (
            <button
              type="button"
              className="job-dismiss-btn"
              onClick={onRemove}
              aria-label={`Dismiss ${job.inputName}`}
              title="Dismiss job"
            >
              ×
            </button>
          )}
        </div>
      </div>

      {isBusy && (
        <progress
          className="job-progress"
          aria-label={`Working on ${title}`}
        />
      )}

      <div className="job-meta-row">
        <p className="job-tool">{TOOLS[job.tool].label}</p>
        <p className="job-sizes">
          <span className="job-size-numbers">
            {size(job.inputSize)} → {size(job.outputSize)}
          </span>
          {sizeDeltaBadge}
          {job.state === 'Saved' && (
            <span className="job-out">
              {' · '}{job.outputName}{job.multi ? ` (${job.outputs?.length} files)` : ''}
            </span>
          )}
        </p>
      </div>

      {job.notice && <p className="job-notice">{job.notice}</p>}

      {job.state === 'NeedsPassword' && job.password && (
        <PasswordForm job={job} onAnswer={onPassword} />
      )}

      {job.error && (
        <div className="job-error">
          <p>{describeError(job.inputNames.length > 1 ? 'these files' : job.inputName, job.error)}</p>
          <details>
            <summary>Details</summary>
            <pre>{[job.error.code, job.error.details].filter(Boolean).join('\n')}</pre>
          </details>
        </div>
      )}

      <div className="job-actions">
        {isBusy && (
          <button type="button" onClick={onCancel} aria-label={`Cancel ${job.inputName}`}>
            Cancel
          </button>
        )}
        {job.state === 'Failed' && (
          <button type="button" className="retry-btn" onClick={onRetry}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
              <path d="M3 3v5h5" />
            </svg>
            Retry
          </button>
        )}
        {job.state === 'Saved' && job.outputs && compatibleTools.length > 0 && onUseInTool && (
          <div className="use-in-tool-group">
            <button type="button" onClick={() => onUseInTool(compatibleTools[0]!, job)}>
              Use in another tool
            </button>
            <select
              aria-label="Choose another tool"
              defaultValue=""
              onChange={(e) => {
                if (e.target.value) onUseInTool(e.target.value as ToolId, job);
                e.target.value = '';
              }}
            >
              <option value="" disabled>
                Choose tool…
              </option>
              {compatibleTools.map((t) => (
                <option key={t} value={t}>
                  {TOOLS[t].label}
                </option>
              ))}
            </select>
          </div>
        )}
        {job.url && (
          <a className="button primary download-btn" href={job.url} download={job.outputName}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
              <polyline points="7 10 12 15 17 10" />
              <line x1="12" y1="15" x2="12" y2="3" />
            </svg>
            Download {job.outputName}
          </a>
        )}
      </div>
    </li>
  );
}
