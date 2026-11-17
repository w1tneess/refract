import { useCallback, useRef, useState } from 'react';
import { createJob, createLimiter, FORMATS, limitsFor, runJob, TOOLS, type Clock, type Job, type Options, type ToolId } from '@refract/core';
import { ENGINE_VERSION, wasmEngine } from '@refract/engine-wasm';
import { downloadZip } from 'client-zip';

export type JobView = Job & { url?: string };

const clock: Clock = {
  now: () => performance.now(),
  every: (ms, fn) => {
    const id = setInterval(fn, ms);
    return () => clearInterval(id);
  },
};
const MAX_SELECTION = limitsFor('image.convert', 'web').maxFiles; // 200 files per drop, as the brief says
const acquire = createLimiter(2); // web concurrency from the brief

const blobOf = (bytes: Uint8Array, mime: string) => new Blob([bytes as BlobPart], { type: mime });

async function urlFor(job: Job): Promise<string | undefined> {
  const outs = job.outputs ?? [];
  if (!outs.length) return undefined;
  if (!job.multi) return URL.createObjectURL(blobOf(outs[0]!.bytes, FORMATS[outs[0]!.format].mime));
  const zip = await downloadZip(outs.map((o) => ({ name: o.name, input: blobOf(o.bytes, FORMATS[o.format].mime) }))).blob();
  return URL.createObjectURL(zip);
}

type Pending = { resolve: (pw: string | null) => void };

export function useJobs() {
  const [jobs, setJobs] = useState<JobView[]>([]);
  const [toast, setToast] = useState('');
  const taken = useRef(new Set<string>());
  const controllers = useRef(new Map<string, AbortController>());
  const files = useRef(new Map<string, File[]>());
  const pending = useRef(new Map<string, Pending>());
  const batchPassword = useRef<string | null>(null);
  const nextId = useRef(1);

  const update = useCallback((job: Job) => {
    const snapshot = { ...job, steps: [...job.steps] };
    setJobs((all) => all.map((j) => (j.id === job.id ? { ...snapshot, url: j.url } : j)));
    if (job.state === 'Saved') {
      void urlFor(job).then((url) => setJobs((all) => all.map((j) => (j.id === job.id ? { ...j, url } : j))));
    }
  }, []);

  const start = useCallback(
    (job: Job, picked: File[]) => {
      const ctl = new AbortController();
      controllers.current.set(job.id, ctl);
      files.current.set(job.id, picked);
      void runJob(
        job,
        picked.map((f) => ({ name: f.name, size: f.size, read: async () => new Uint8Array(await f.arrayBuffer()) })),
        {
          engine: wasmEngine,
          engineVersion: ENGINE_VERSION,
          platform: 'web',
          clock,
          isPaused: () => document.visibilityState === 'hidden',
          signal: ctl.signal,
          acquire,
          // "Use for all locked files": try the remembered password first; ask again only if it fails.
          askPassword: (j, _file, wrong) =>
            !wrong && batchPassword.current !== null
              ? Promise.resolve(batchPassword.current)
              : new Promise((resolve) => pending.current.set(j.id, { resolve })),
          onUpdate: update,
        },
      ).finally(() => controllers.current.delete(job.id));
    },
    [update],
  );

  const addFiles = useCallback(
    (tool: ToolId, options: Options, picked: File[]) => {
      if (!picked.length) return;
      const chosen = picked.slice(0, MAX_SELECTION);
      setToast(
        picked.length > chosen.length
          ? `Added the first ${chosen.length} of ${picked.length} files. You can add up to ${MAX_SELECTION} at a time.`
          : '',
      );
      // Tools that combine files (merge, images to PDF) make one job per drop; others one job per file.
      const groups = TOOLS[tool].inputs === 'many' ? [chosen] : chosen.map((f) => [f]);
      const created = groups.map((g) => ({
        job: createJob(String(nextId.current++), tool, options, g.map((f) => ({ name: f.name, size: f.size })), taken.current),
        g,
      }));
      setJobs((all) => [...all, ...created.map(({ job }) => ({ ...job }))]);
      for (const { job, g } of created) start(job, g);
    },
    [start],
  );

  const cancel = useCallback((id: string) => controllers.current.get(id)?.abort(), []);

  /** Re-queue a failed job as a fresh job with the same files and options, in the same place. */
  const retry = useCallback(
    (id: string) => {
      const picked = files.current.get(id);
      setJobs((all) => {
        const old = all.find((j) => j.id === id);
        if (!old || !picked) return all;
        const job = createJob(String(nextId.current++), old.tool, old.options, picked.map((f) => ({ name: f.name, size: f.size })), taken.current);
        queueMicrotask(() => start(job, picked));
        return all.map((j) => (j.id === id ? { ...job } : j));
      });
    },
    [start],
  );

  const answerPassword = useCallback((id: string, password: string | null, forAll: boolean) => {
    if (forAll) batchPassword.current = password;
    const p = pending.current.get(id);
    pending.current.delete(id);
    p?.resolve(password);
  }, []);

  const saveAllUrl = useCallback(async (all: JobView[]) => {
    const entries = all
      .filter((j) => j.state === 'Saved' && j.outputs)
      .flatMap((j) =>
        j.multi
          ? j.outputs!.map((o) => ({ name: `${j.outputName.replace(/\.zip$/, '')}/${o.name}`, input: blobOf(o.bytes, FORMATS[o.format].mime) }))
          : [{ name: j.outputName, input: blobOf(j.outputs![0]!.bytes, FORMATS[j.outputs![0]!.format].mime) }],
      );
    return URL.createObjectURL(await downloadZip(entries).blob());
  }, []);

  return { jobs, toast, addFiles, cancel, retry, answerPassword, saveAllUrl };
}
