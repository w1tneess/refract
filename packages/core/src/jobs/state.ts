export type JobState = 'Queued' | 'Probing' | 'Running' | 'Verifying' | 'Saved' | 'Rejected' | 'Failed' | 'Canceled';

// Canceled is its own end state, reachable from any non-terminal state; it is not a failure.
const NEXT: Record<JobState, JobState[]> = {
  Queued: ['Probing', 'Canceled'],
  Probing: ['Running', 'Rejected', 'Canceled'],
  Running: ['Verifying', 'Failed', 'Canceled'],
  Verifying: ['Saved', 'Failed', 'Canceled'],
  Saved: [],
  Rejected: [],
  Failed: [],
  Canceled: [],
};

export function canMove(from: JobState, to: JobState): boolean {
  return NEXT[from].includes(to);
}

export function isTerminal(s: JobState): boolean {
  return NEXT[s].length === 0;
}

export type BatchStatus = 'processing' | 'waiting' | 'error' | 'finished';

/** Same priority as CloudConvert: processing, then waiting, then error, then finished. */
export function batchStatus(states: JobState[]): BatchStatus {
  if (states.some((s) => s === 'Probing' || s === 'Running' || s === 'Verifying')) return 'processing';
  if (states.includes('Queued')) return 'waiting';
  if (states.some((s) => s === 'Failed' || s === 'Rejected')) return 'error';
  return 'finished';
}
