interface TelemetryBarProps {
  activeJobsCount: number;
  engineLabel: string;
  memoryEstimateMb: number;
  hasJobs: boolean;
  onReRunQueue?: () => void;
}

export function TelemetryBar({
  activeJobsCount,
  engineLabel,
  memoryEstimateMb,
  hasJobs,
  onReRunQueue,
}: TelemetryBarProps) {
  return (
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
          <span className="telemetry-label">Concurrency:</span>
          <span className="telemetry-val">
            {activeJobsCount}/2 workers active
          </span>
        </div>

        <div className="telemetry-col telemetry-col-engine">
          <span className="telemetry-label">Engine:</span>
          <span className="telemetry-val">{engineLabel}</span>
        </div>

        <div className="telemetry-col">
          <span className="telemetry-label">Est. memory:</span>
          <span className="telemetry-val">~{memoryEstimateMb} MB</span>
        </div>

        <div className="telemetry-col telemetry-col-version">
          <span className="telemetry-val">v1.0.0</span>
        </div>

        {hasJobs && onReRunQueue && (
          <div className="telemetry-col telemetry-action-col">
            <button
              type="button"
              className="telemetry-rerun-btn"
              onClick={onReRunQueue}
              title="Re-run all jobs in memory with current settings"
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
  );
}
