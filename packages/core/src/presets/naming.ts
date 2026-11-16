export const CONVERTED = '{name} (converted)';

/**
 * Build an output file name from a template and make it unique against `taken`
 * (names already given out in this batch/session), adding " (2)", " (3)", …
 * Adapted from Tichau File Converter's GenerateUniquePath (GPL-3.0).
 */
export function outputName(template: string, inputName: string, ext: string, taken: Set<string>): string {
  const dot = inputName.lastIndexOf('.');
  const stem = dot > 0 ? inputName.slice(0, dot) : inputName;
  const base = template.replaceAll('{name}', stem);
  let name = `${base}.${ext}`;
  // Case-insensitive: Windows and macOS treat "A.jpg" and "a.jpg" as the same file.
  for (let n = 2; taken.has(name.toLowerCase()); n++) name = `${base} (${n}).${ext}`;
  taken.add(name.toLowerCase());
  return name;
}
