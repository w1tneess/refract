import { useEffect, useId, useState, type DragEvent } from 'react';
import { batchStatus, FORMATS, resolveOptions, TOOLS, type Kind, type Options, type ToolId } from '@refract/core';
import { JobCard } from './JobCard.tsx';
import { OptionsForm } from './OptionsForm.tsx';
import { useJobs } from './useJobs.ts';

const GROUPS: { label: string; tools: ToolId[] }[] = [
  { label: 'Images', tools: ['convert-image', 'compress-image', 'resize-image'] },
  { label: 'PDF', tools: ['merge-pdf', 'split-pdf', 'reorder-pdf', 'images-to-pdf', 'pdf-to-images', 'compress-pdf'] },
  { label: 'Audio', tools: ['convert-audio', 'trim-audio'] },
];
const ACCEPT: Record<Kind, string> = Object.fromEntries(
  (['image', 'pdf', 'audio'] as Kind[]).map((k) => [k, Object.values(FORMATS).filter((f) => f.kind === k).flatMap((f) => [...f.ext.map((e) => `.${e}`), f.mime]).join(',')]),
) as Record<Kind, string>;

const toolFromHash = (): ToolId => {
  const h = location.hash.slice(1) as ToolId;
  return h in TOOLS ? h : 'convert-image';
};

// Remembered options per tool: a per-viewer convenience, so storage failures are ignored.
const loadOptions = (tool: ToolId): Options => {
  try {
    return resolveOptions(TOOLS[tool].options, JSON.parse(localStorage.getItem(`refract.options.${tool}`) ?? '{}'));
  } catch {
    return resolveOptions(TOOLS[tool].options);
  }
};
const saveOptions = (tool: ToolId, o: Options) => {
  try { localStorage.setItem(`refract.options.${tool}`, JSON.stringify(o)); } catch { /* private mode */ }
};

export function App() {
  const { jobs, toast, addFiles, cancel, retry, answerPassword, saveAllUrl } = useJobs();
  const [tool, setTool] = useState<ToolId>(toolFromHash);
  const [options, setOptions] = useState<Options>(() => loadOptions(toolFromHash()));
  const [over, setOver] = useState(false);
  const [allUrl, setAllUrl] = useState<string>();
  const inputId = useId();
  const t = TOOLS[tool];

  useEffect(() => {
    const onHash = () => { const next = toolFromHash(); setTool(next); setOptions(loadOptions(next)); };
    addEventListener('hashchange', onHash);
    return () => removeEventListener('hashchange', onHash);
  }, []);

  const changeOptions = (o: Options) => { setOptions(o); saveOptions(tool, resolveOptions(t.options, o)); };
  const add = (files: File[]) => addFiles(tool, resolveOptions(t.options, options), files);
  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    setOver(false);
    add([...e.dataTransfer.files]);
  };
  const status = jobs.length ? batchStatus(jobs.map((j) => j.state)) : null;
  const saved = jobs.filter((j) => j.state === 'Saved').length;

  return (
    <main>
      <header>
        <h1>Refract</h1>
        <p>Convert images, PDFs and audio. Files never leave your device.</p>
      </header>
      <nav aria-label="Tools" className="tools">
        {GROUPS.map((g) => (
          <div key={g.label} className="tool-group">
            <h2>{g.label}</h2>
            <ul>
              {g.tools.map((id) => (
                <li key={id}><a href={`#${id}`} aria-current={id === tool ? 'page' : undefined}>{TOOLS[id].label}</a></li>
              ))}
            </ul>
          </div>
        ))}
      </nav>
      <section aria-labelledby="tool-title">
        <h2 id="tool-title">{t.label}</h2>
        <p className="blurb">{t.blurb}</p>
        <OptionsForm specs={t.options} values={options} onChange={changeOptions}
          onReset={() => changeOptions(resolveOptions(t.options))} />
        <label
          htmlFor={inputId}
          className={`dropzone${over ? ' over' : ''}`}
          data-testid="dropzone"
          onDragOver={(e) => { e.preventDefault(); setOver(true); }}
          onDragLeave={() => setOver(false)}
          onDrop={onDrop}
        >
          <strong>Drop {t.accepts === 'image' ? 'images' : t.accepts === 'pdf' ? 'PDFs' : 'audio files'} here</strong>
          <span>or click to choose{t.inputs === 'many' ? ' (files are combined in the order chosen)' : ''}</span>
          <input
            id={inputId}
            className="visually-hidden"
            type="file"
            multiple
            accept={ACCEPT[t.accepts]}
            onChange={(e) => { add([...(e.target.files ?? [])]); e.target.value = ''; }}
          />
        </label>
      </section>
      <div role="status" aria-live="polite" className="toast">{toast}</div>
      {status && (
        <div className="batch">
          <p data-batch={status}>Batch: {status}</p>
          {saved > 1 && (
            allUrl
              ? <a className="button" href={allUrl} download="refract.zip" onClick={() => setTimeout(() => setAllUrl(undefined), 0)}>Download refract.zip</a>
              : <button type="button" onClick={() => void saveAllUrl(jobs).then(setAllUrl)}>Save all as ZIP</button>
          )}
        </div>
      )}
      <ul className="jobs" aria-label="Jobs">
        {jobs.map((j) => (
          <JobCard key={j.id} job={j} onCancel={() => cancel(j.id)} onRetry={() => retry(j.id)}
            onPassword={(pw, all) => answerPassword(j.id, pw, all)} />
        ))}
      </ul>
    </main>
  );
}
