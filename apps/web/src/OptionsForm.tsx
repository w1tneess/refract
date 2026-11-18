import { useId } from 'react';
import type { OptionSpec, Options } from '@refract/core';

const visible = (spec: OptionSpec, values: Options) =>
  !spec.showIf || spec.showIf.is.includes(values[spec.showIf.key] as string);

/** The settings panel, built from a tool's OptionSpec data. */
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
      <legend className="visually-hidden">Settings</legend>

      <div className="options-header">
        <span className="options-hint">Applies to new files</span>
        <button type="button" className="link reset-btn" onClick={onReset} title="Restore default settings">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
            <path d="M3 3v5h5" />
          </svg>
          Reset to defaults
        </button>
      </div>

      <div className="options-grid">
        {specs
          .filter((spec) => visible(spec, values))
          .map((spec) => {
            const fieldId = `${id}-${spec.key}`;
            const helpId = spec.help ? `${fieldId}-help` : undefined;
            return (
              <div className="option" key={spec.key}>
                <label htmlFor={fieldId} className="option-label">
                  <span>{spec.label}</span>
                  {spec.type === 'number' && spec.unit && <span className="option-unit">{spec.unit}</span>}
                </label>
                <div className="option-control">
                  {spec.type === 'select' ? (
                    <div className="select-wrap">
                      <select
                        id={fieldId}
                        name={spec.key}
                        value={String(values[spec.key])}
                        aria-describedby={helpId}
                        onChange={(e) => set(spec.key, e.target.value)}
                      >
                        {spec.choices.map((choice) => (
                          <option key={choice.value} value={choice.value}>
                            {choice.label}
                          </option>
                        ))}
                      </select>
                      <svg className="select-chevron" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                        <polyline points="6 9 12 15 18 9" />
                      </svg>
                    </div>
                  ) : spec.type === 'number' ? (
                    <input
                      id={fieldId}
                      name={spec.key}
                      type="number"
                      inputMode={spec.step && spec.step < 1 ? 'decimal' : 'numeric'}
                      autoComplete="off"
                      min={spec.min}
                      max={spec.max}
                      step={spec.step ?? 1}
                      value={values[spec.key]}
                      aria-describedby={helpId}
                      onChange={(e) => set(spec.key, e.target.value === '' ? '' : Number(e.target.value))}
                    />
                  ) : (
                    <input
                      id={fieldId}
                      name={spec.key}
                      type="text"
                      autoComplete="off"
                      spellCheck={false}
                      value={String(values[spec.key])}
                      placeholder={spec.placeholder}
                      aria-describedby={helpId}
                      onChange={(e) => set(spec.key, e.target.value)}
                    />
                  )}
                </div>
                {spec.help && (
                  <small id={helpId} className="option-help">
                    {spec.help}
                  </small>
                )}
              </div>
            );
          })}
      </div>
    </fieldset>
  );
}
