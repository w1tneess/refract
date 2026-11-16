import { useCallback, useRef, useState } from 'react';
import { createJob, createLimiter, limitsFor, runJob, type Clock, type Job } from '@refract/core';
import { wasmEngine } from '@refract/engine-wasm';

export type JobView = Job & { url?: string };

const clock: Clock = {
  now: () => performance.now(),
  every: (ms, fn) => {
    const id = setInterval(fn, ms);
    return () => clearInterval(id);
  },
};
const LIMITS = limitsFor('image.convert', 'web');
const acquire = createLimiter(2); // web concurrency from the brief

export function useJobs() {
  const [jobs, setJobs] = useState<JobView[]>([]);
  const [toast, setToast] = useState('');
  const taken = useRef(new Set<string>());
  const controllers = useRef(new Map<string, AbortController>());
  const nextId = useRef(1);

  const update = useCallback((job: Job) => {
    // Hand the output to the browser as a Blob URL, then drop our copy of the bytes.
    let url: string | undefined;
    if (job.state === 'Saved' && job.output) {
      url = URL.createObjectURL(new Blob([job.output as BlobPart], { type: 'image/jpeg' }));
      job.output = undefined;
    }
    setJobs((all) => all.map((j) => (j.id === job.id ? { ...job, steps: [...job.steps], url: url ?? j.url } : j)));
  }, []);

  const addFiles = useCallback(
    (picked: File[]) => {
      if (!picked.length) return;
      const files = picked.slice(0, LIMITS.maxFiles);
      setToast(
        picked.length > files.length
          ? `Added the first ${files.length} of ${picked.length} files. You can add up to ${LIMITS.maxFiles} at a time.`
          : '',
      );
      const created = files.map((file) => {
        const job = createJob(String(nextId.current++), file.name, file.size, 'jpeg', taken.current);
        return { job, file };
      });
      setJobs((all) => [...all, ...created.map(({ job }) => ({ ...job }))]);
      for (const { job, file } of created) {
        const ctl = new AbortController();
        controllers.current.set(job.id, ctl);
        void runJob(job, async () => new Uint8Array(await file.arrayBuffer()), {
          engine: wasmEngine,
          platform: 'web',
          op: 'image.convert',
          target: 'jpeg',
          clock,
          isPaused: () => document.visibilityState === 'hidden',
          signal: ctl.signal,
          acquire,
          onUpdate: update,
        }).finally(() => controllers.current.delete(job.id));
      }
    },
    [update],
  );

  const cancel = useCallback((id: string) => controllers.current.get(id)?.abort(), []);

  return { jobs, toast, addFiles, cancel };
}
