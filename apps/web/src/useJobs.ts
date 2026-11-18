import { useCallback, useEffect, useRef, useState } from 'react';
import { createJob, createLimiter, FORMATS, isTerminal, limitsFor, runJob, TOOLS, type Clock, type Job, type Options, type ToolId } from '@refract/core';
import { ENGINE_VERSION, wasmEngine } from '@refract/engine-wasm';
import { downloadZip } from 'client-zip';

export type JobView = Job & { url?: string; originalUrl?: string };

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
const revokeUrl = (url?: string) => {
  if (url) {
    try {
      URL.revokeObjectURL(url);
    } catch {
      /* already revoked */
    }
  }
};

/** Browser <img> elements can render standard web image formats, but not HEIC/HEIF. */
const isBrowserRenderableImage = (f: File) => {
  if (/\.heic$|\.heif$/i.test(f.name) || f.type === 'image/heic' || f.type === 'image/heif') return false;
  return f.type.startsWith('image/') || /\.(png|jpe?g|webp|avif|gif)$/i.test(f.name);
};

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
  const jobsRef = useRef<JobView[]>([]);
  const taken = useRef(new Set<string>());
  const controllers = useRef(new Map<string, AbortController>());
  const files = useRef(new Map<string, File[]>());
  const pending = useRef(new Map<string, Pending>());
  const batchPassword = useRef<string | null>(null);
  const nextId = useRef(1);

  jobsRef.current = jobs;

  // Revoke any remaining blob URLs and abort active workers on unmount.
  useEffect(() => {
    const activeControllers = controllers.current;
    return () => {
      for (const ctl of activeControllers.values()) ctl.abort();
      for (const j of jobsRef.current) {
        revokeUrl(j.url);
        revokeUrl(j.originalUrl);
      }
    };
  }, []);

  const update = useCallback((job: Job) => {
    const snapshot = { ...job, steps: [...job.steps] };
    setJobs((all) => all.map((j) => (j.id === job.id ? { ...snapshot, url: j.url, originalUrl: j.originalUrl } : j)));
    if (job.state === 'Saved') {
      void urlFor(job).then((url) => {
        if (!url) return;
        let attached = false;
        setJobs((all) =>
          all.map((j) => {
            if (j.id !== job.id) return j;
            attached = true;
            revokeUrl(j.url);
            return { ...j, url };
          }),
        );
        // If the job was removed before urlFor resolved, avoid leaking the newly created blob URL.
        queueMicrotask(() => {
          if (!attached && !jobsRef.current.some((j) => j.id === job.id)) revokeUrl(url);
        });
      });
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
      ).finally(() => {
        controllers.current.delete(job.id);
        pending.current.delete(job.id);
      });
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
      const created = groups.map((g) => {
        let origUrl: string | undefined;
        if (g.length === 1 && isBrowserRenderableImage(g[0]!)) {
          try {
            origUrl = URL.createObjectURL(g[0]!);
          } catch {}
        }
        return {
          job: createJob(String(nextId.current++), tool, options, g.map((f) => ({ name: f.name, size: f.size })), taken.current),
          origUrl,
          g,
        };
      });
      setJobs((all) => [...all, ...created.map(({ job, origUrl }) => ({ ...job, originalUrl: origUrl }))]);
      for (const { job, g } of created) start(job, g);
    },
    [start],
  );

  const cancel = useCallback((id: string) => {
    controllers.current.get(id)?.abort();
    const p = pending.current.get(id);
    if (p) {
      pending.current.delete(id);
      p.resolve(null);
    }
  }, []);

  /** Re-queue a failed job as a fresh job with the same files and options, in the same place. */
  const retry = useCallback(
    (id: string) => {
      const old = jobsRef.current.find((j) => j.id === id);
      const picked = files.current.get(id);
      if (!old || !picked) return;
      revokeUrl(old.url);
      files.current.delete(id);
      const job = createJob(String(nextId.current++), old.tool, old.options, picked.map((f) => ({ name: f.name, size: f.size })), taken.current);
      const origUrl = old.originalUrl;
      setJobs((all) => all.map((j) => (j.id === id ? { ...job, originalUrl: origUrl } : j)));
      start(job, picked);
    },
    [start],
  );

  /** Re-run existing jobs with new options. When a tool is given, leave other tools' jobs alone. */
  const reRunAll = useCallback(
    (targetTool?: ToolId, newOptions?: Options) => {
      const toStart: { oldId: string; job: Job; picked: File[]; origUrl?: string }[] = [];
      for (const old of jobsRef.current) {
        if (targetTool && old.tool !== targetTool) continue;
        const picked = files.current.get(old.id);
        if (!picked) continue;
        controllers.current.get(old.id)?.abort();
        const p = pending.current.get(old.id);
        if (p) {
          pending.current.delete(old.id);
          p.resolve(null);
        }
        revokeUrl(old.url);
        files.current.delete(old.id);
        const t = targetTool ?? old.tool;
        const o = newOptions ?? old.options;
        const job = createJob(String(nextId.current++), t, o, picked.map((f) => ({ name: f.name, size: f.size })), taken.current);
        toStart.push({ oldId: old.id, job, picked, origUrl: old.originalUrl });
      }
      if (!toStart.length) return;
      const replacements = new Map(toStart.map((entry) => [entry.oldId, { ...entry.job, originalUrl: entry.origUrl }]));
      setJobs((all) => all.map((j) => replacements.get(j.id) ?? j));
      for (const { job, picked } of toStart) start(job, picked);
    },
    [start],
  );

  const answerPassword = useCallback((id: string, password: string | null, forAll: boolean) => {
    if (forAll && password !== null) batchPassword.current = password;
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

  const clearJobs = useCallback(() => {
    const remaining: JobView[] = [];
    for (const j of jobsRef.current) {
      if (isTerminal(j.state)) {
        revokeUrl(j.url);
        revokeUrl(j.originalUrl);
        files.current.delete(j.id);
      } else {
        remaining.push(j);
      }
    }
    if (remaining.length === 0) {
      batchPassword.current = null;
    }
    setJobs((all) => all.filter((j) => !isTerminal(j.state)));
  }, []);

  const removeJob = useCallback((id: string) => {
    controllers.current.get(id)?.abort();
    const p = pending.current.get(id);
    if (p) {
      pending.current.delete(id);
      p.resolve(null);
    }
    const target = jobsRef.current.find((j) => j.id === id);
    if (target) {
      revokeUrl(target.url);
      revokeUrl(target.originalUrl);
    }
    files.current.delete(id);
    setJobs((all) => all.filter((j) => j.id !== id));
  }, []);

  return { jobs, toast, addFiles, cancel, retry, reRunAll, answerPassword, saveAllUrl, clearJobs, removeJob };
}
