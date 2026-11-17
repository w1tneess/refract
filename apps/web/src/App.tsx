import { useId, useState, type DragEvent } from 'react';
import { batchStatus, describeError, isTerminal } from '@refract/core';
import { useJobs, type JobView } from './useJobs.ts';

const kb = (n?: number) =>
  n === undefined
    ? '—'
    : n < 1024 * 1024
      ? `${Math.max(1, Math.round(n / 1024))} KB`
      : `${(n / 1024 / 1024).toFixed(1)} MB`;

const STATE_LABEL: Record<JobView['state'], string> = {
  Queued: 'Waiting',
  Probing: 'Checking',
  Running: 'Converting',
  Verifying: 'Verifying',
  Saved: 'Done',
  Rejected: "Can't convert",
  Failed: 'Failed',
  Canceled: 'Canceled',
};

function JobCard({ job, onCancel }: { job: JobView; onCancel: () => void }) {
  const isBusy = !isTerminal(job.state);
  const diff = job.inputSize && job.outputSize ? job.inputSize - job.outputSize : 0;
  const pct =
    job.inputSize && job.outputSize && job.inputSize > 0
      ? Math.round((diff / job.inputSize) * 100)
      : 0;

  return (
    <li
      className={`job job-${job.state.toLowerCase()}`}
      data-state={job.state}
      data-timings={JSON.stringify(job.timings)}
    >
      <div className="job-head">
        <span className="job-name" title={job.inputName}>
          {job.inputName}
        </span>
        <span className="job-state" role="status">
          {STATE_LABEL[job.state]}
        </span>
      </div>

      {isBusy && (
        <progress
          className="job-progress"
          aria-label={`Converting ${job.inputName}`}
        />
      )}

      {job.notice && <p className="job-notice">{job.notice}</p>}

      <p className="job-sizes">
        <span className="job-size-numbers">
          {kb(job.inputSize)} → {kb(job.outputSize)}
        </span>
        {job.state === 'Saved' && pct > 0 && (
          <span className="job-savings">(-{pct}%)</span>
        )}
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
        {isBusy && (
          <button
            type="button"
            onClick={onCancel}
            aria-label={`Cancel ${job.inputName}`}
          >
            Cancel
          </button>
        )}
        {job.url && (
          <a className="button primary" href={job.url} download={job.outputName}>
            Download {job.outputName}
          </a>
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
      <header className="app-header">
        <h1>Refract</h1>
        <p>Convert HEIC photos to JPEG on your device. Nothing leaves your browser.</p>
      </header>

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
        <strong className="dropzone-title">Drop HEIC photos here</strong>
        <span className="dropzone-subtitle">or choose files</span>
        <input
          id={inputId}
          className="visually-hidden"
          type="file"
          multiple
          accept=".heic,.heif,image/heic,image/heif"
          onChange={(e) => {
            addFiles([...(e.target.files ?? [])]);
            e.target.value = '';
          }}
        />
      </label>

      <div role="status" aria-live="polite" className="toast">
        {toast}
      </div>

      {status && (
        <div className="batch-row">
          <p className="batch" data-batch={status}>
            Batch: {status}
          </p>
          <span className="batch-count">
            {jobs.length} {jobs.length === 1 ? 'file' : 'files'}
          </span>
        </div>
      )}

      <ul className="jobs" aria-label="Conversions">
        {jobs.map((j) => (
          <JobCard key={j.id} job={j} onCancel={() => cancel(j.id)} />
        ))}
      </ul>
    </main>
  );
}