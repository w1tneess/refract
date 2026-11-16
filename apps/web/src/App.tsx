import { useId, useState, type DragEvent } from 'react';
import { batchStatus, describeError, isTerminal } from '@refract/core';
import { useJobs, type JobView } from './useJobs.ts';

const kb = (n?: number) => (n === undefined ? '—' : n < 1024 * 1024 ? `${Math.max(1, Math.round(n / 1024))} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`);

const STATE_LABEL: Record<JobView['state'], string> = {
  Queued: 'Waiting',
  Probing: 'Checking file',
  Running: 'Converting',
  Verifying: 'Checking result',
  Saved: 'Done',
  Rejected: "Can't convert",
  Failed: 'Failed',
  Canceled: 'Canceled',
};

function JobCard({ job, onCancel }: { job: JobView; onCancel: () => void }) {
  return (
    <li className={`job job-${job.state.toLowerCase()}`} data-state={job.state} data-timings={JSON.stringify(job.timings)}>
      <div className="job-head">
        <span className="job-name" title={job.inputName}>{job.inputName}</span>
        <span className="job-state" role="status">{STATE_LABEL[job.state]}</span>
      </div>
      {job.notice && <p className="job-notice">{job.notice}</p>}
      <p className="job-sizes">
        {kb(job.inputSize)} → {kb(job.outputSize)}
        {job.state === 'Saved' && <span className="job-out"> · {job.outputName}</span>}
      </p>
      {job.error && (
        <div className="job-error">
          <p>{describeError(job.inputName, job.error)}</p>
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
        {job.url && (
          <a className="button primary" href={job.url} download={job.outputName}>Download {job.outputName}</a>
        )}
      </div>
    </li>
  );
}

export function App() {
  const { jobs, toast, addFiles, cancel } = useJobs();
  const [over, setOver] = useState(false);
  const inputId = useId();
  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    setOver(false);
    addFiles([...e.dataTransfer.files]);
  };
  const status = jobs.length ? batchStatus(jobs.map((j) => j.state)) : null;

  return (
    <main>
      <header>
        <h1>Refract</h1>
        <p>Convert HEIC photos to JPG. Files never leave your device.</p>
      </header>
      <label
        htmlFor={inputId}
        className={`dropzone${over ? ' over' : ''}`}
        data-testid="dropzone"
        onDragOver={(e) => { e.preventDefault(); setOver(true); }}
        onDragLeave={() => setOver(false)}
        onDrop={onDrop}
      >
        <strong>Drop HEIC photos here</strong>
        <span>or click to choose files</span>
        <input
          id={inputId}
          className="visually-hidden"
          type="file"
          multiple
          accept=".heic,.heif,image/heic,image/heif"
          onChange={(e) => { addFiles([...(e.target.files ?? [])]); e.target.value = ''; }}
        />
      </label>
      <div role="status" aria-live="polite" className="toast">{toast}</div>
      {status && <p className="batch" data-batch={status}>Batch: {status}</p>}
      <ul className="jobs" aria-label="Conversions">
        {jobs.map((j) => <JobCard key={j.id} job={j} onCancel={() => cancel(j.id)} />)}
      </ul>
    </main>
  );
}
