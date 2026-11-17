import { useId } from 'react';
import type { OptionSpec, Options } from '@refract/core';

const visible = (s: OptionSpec, values: Options) => !s.showIf || s.showIf.is.includes(values[s.showIf.key] as string);

/** The options panel, built from a tool's OptionSpec data. */
export function OptionsForm({ specs, values, onChange, onReset }: {
  specs: OptionSpec[];
  values: Options;
  onChange: (v: Options) => void;
  onReset: () => void;
}) {
  const id = useId();
  if (!specs.length) return null;
  const set = (key: string, v: number | string) => onChange({ ...values, [key]: v });
  return (
    <fieldset className="options">
      <legend>Options</legend>
      {specs.filter((s) => visible(s, values)).map((s) => {
        const fid = `${id}-${s.key}`;
        const help = s.help ? `${fid}-help` : undefined;
        return (
          <div className="option" key={s.key}>
            <label htmlFor={fid}>{s.label}{s.type === 'number' && s.unit ? ` (${s.unit})` : ''}</label>
            {s.type === 'select' ? (
              <select id={fid} value={String(values[s.key])} aria-describedby={help} onChange={(e) => set(s.key, e.target.value)}>
                {s.choices.map((c) => <option key={c.value} value={c.value}>{c.label}</option>)}
              </select>
            ) : s.type === 'number' ? (
              <input id={fid} type="number" min={s.min} max={s.max} step={s.step ?? 1} value={values[s.key]} aria-describedby={help}
                onChange={(e) => set(s.key, e.target.value === '' ? '' : Number(e.target.value))} />
            ) : (
              <input id={fid} type="text" value={String(values[s.key])} placeholder={s.placeholder} aria-describedby={help}
                onChange={(e) => set(s.key, e.target.value)} />
            )}
            {s.help && <small id={help}>{s.help}</small>}
          </div>
        );
      })}
      <button type="button" className="link" onClick={onReset}>Reset to defaults</button>
    </fieldset>
  );
}
