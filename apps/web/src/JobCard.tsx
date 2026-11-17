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

function PasswordForm({ job, onAnswer }: { job: JobView; onAnswer: (pw: string | null, forAll: boolean) => void }) {
  const id = useId();
  const [pw, setPw] = useState('');
  const [all, setAll] = useState(false);
  return (
    <form className="password" onSubmit={(e) => { e.preventDefault(); onAnswer(pw, all); }}>
      <label htmlFor={`${id}-pw`}>
        {job.password!.file} is password-protected{job.password!.wrong ? '. That password is wrong; try again' : ''}.
      </label>
      <input id={`${id}-pw`} type="password" autoComplete="off" value={pw} onChange={(e) => setPw(e.target.value)}
        aria-invalid={job.password!.wrong || undefined} />
      <label className="check"><input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} /> Use for all locked files</label>
      <div className="job-actions">
        <button type="submit" className="primary">Unlock</button>
        <button type="button" onClick={() => onAnswer(null, all)}>Skip this file</button>
      </div>
    </form>
  );
}

export function JobCard({ job, onCancel, onRetry, onPassword, onUseInTool }: {
  job: JobView;
  onCancel: () => void;
  onRetry: () => void;
  onPassword: (pw: string | null, forAll: boolean) => void;
  onUseInTool?: (targetTool: ToolId, job: JobView) => void;
}) {
  const title = job.inputNames.length > 1 ? `${job.inputName} + ${job.inputNames.length - 1} more` : job.inputName;
  const outKind = job.outputs?.[0] ? FORMATS[job.outputs[0]!.format].kind : null;
  const compatibleTools = outKind ? (Object.keys(TOOLS) as ToolId[]).filter((id) => TOOLS[id].accepts === outKind && id !== job.tool) : [];
  return (
    <li className={`job job-${job.state.toLowerCase()}`} data-state={job.state} data-timings={JSON.stringify(job.timings)}>
      <div className="job-head">
        <span className="job-name" title={job.inputNames.join('\n')}>{title}</span>
        <span className="job-state" role="status">{STATE_LABEL[job.state]}</span>
      </div>
      <p className="job-tool">{TOOLS[job.tool].label}</p>
      {job.notice && <p className="job-notice">{job.notice}</p>}
      <p className="job-sizes">
        {size(job.inputSize)} → {size(job.outputSize)}
        {job.state === 'Saved' && <span className="job-out"> · {job.outputName}{job.multi ? ` (${job.outputs?.length} files)` : ''}</span>}
      </p>
      {job.state === 'NeedsPassword' && job.password && <PasswordForm job={job} onAnswer={onPassword} />}
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
        {!isTerminal(job.state) && (
          <button type="button" onClick={onCancel} aria-label={`Cancel ${job.inputName}`}>Cancel</button>
        )}
        {job.state === 'Failed' && <button type="button" onClick={onRetry}>Retry</button>}
        {job.state === 'Saved' && job.outputs && compatibleTools.length > 0 && onUseInTool && (
          <div className="use-in-tool-group">
            <button type="button" onClick={() => onUseInTool(compatibleTools[0]!, job)}>Use in another tool</button>
            <select
              aria-label="Choose another tool"
              defaultValue=""
              onChange={(e) => {
                if (e.target.value) onUseInTool(e.target.value as ToolId, job);
                e.target.value = '';
              }}
            >
              <option value="" disabled>Choose tool…</option>
              {compatibleTools.map((t) => (
                <option key={t} value={t}>{TOOLS[t].label}</option>
              ))}
            </select>
          </div>
        )}
        {job.url && <a className="button primary" href={job.url} download={job.outputName}>Download {job.outputName}</a>}
      </div>
    </li>
  );
}
