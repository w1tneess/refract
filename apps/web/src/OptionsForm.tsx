import { useId } from 'react';
import type { OptionSpec, Options } from '@refract/core';

const visible = (s: OptionSpec, values: Options) => !s.showIf || s.showIf.is.includes(values[s.showIf.key] as string);

/** The options panel, built from a tool's OptionSpec data. */
export function OptionsForm({
  specs,
  values,
  onChange,
  onReset,
}: {
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
      <div className="options-header">
        <legend className="options-legend">
          <svg className="options-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <line x1="4" y1="21" x2="4" y2="14" />
            <line x1="4" y1="10" x2="4" y2="3" />
            <line x1="12" y1="21" x2="12" y2="12" />
            <line x1="12" y1="8" x2="12" y2="3" />
            <line x1="20" y1="21" x2="20" y2="16" />
            <line x1="20" y1="12" x2="20" y2="3" />
            <line x1="1" y1="14" x2="7" y2="14" />
            <line x1="9" y1="8" x2="15" y2="8" />
            <line x1="17" y1="16" x2="23" y2="16" />
          </svg>
          Options
        </legend>
        <button type="button" className="link reset-btn" onClick={onReset} title="Restore default settings">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
            <path d="M3 3v5h5" />
          </svg>
          Reset to defaults
        </button>
      </div>

      <div className="options-grid">
        {specs.filter((s) => visible(s, values)).map((s) => {
          const fid = `${id}-${s.key}`;
          const help = s.help ? `${fid}-help` : undefined;
          return (
            <div className="option" key={s.key}>
              <label htmlFor={fid} className="option-label">
                {s.label}
                {s.type === 'number' && s.unit && <span className="option-unit">({s.unit})</span>}
              </label>
              <div className="option-control">
                {s.type === 'select' ? (
                  <div className="select-wrap">
                    <select
                      id={fid}
                      value={String(values[s.key])}
                      aria-describedby={help}
                      onChange={(e) => set(s.key, e.target.value)}
                    >
                      {s.choices.map((c) => (
                        <option key={c.value} value={c.value}>
                          {c.label}
                        </option>
                      ))}
                    </select>
                    <svg className="select-chevron" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                      <polyline points="6 9 12 15 18 9" />
                    </svg>
                  </div>
                ) : s.type === 'number' ? (
                  <input
                    id={fid}
                    type="number"
                    min={s.min}
                    max={s.max}
                    step={s.step ?? 1}
                    value={values[s.key]}
                    aria-describedby={help}
                    onChange={(e) => set(s.key, e.target.value === '' ? '' : Number(e.target.value))}
                  />
                ) : (
                  <input
                    id={fid}
                    type="text"
                    value={String(values[s.key])}
                    placeholder={s.placeholder}
                    aria-describedby={help}
                    onChange={(e) => set(s.key, e.target.value)}
                  />
                )}
              </div>
              {s.help && (
                <small id={help} className="option-help">
                  {s.help}
                </small>
              )}
            </div>
          );
        })}
      </div>
    </fieldset>
  );
}
