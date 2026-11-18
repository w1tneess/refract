import { RefractError } from '../errors.ts';

const bad = (why: string, todo: string) => new RefractError('Unsupported', why, todo);

/**
 * Parse "1-3, 5, 7-" or "1-3, 5, 7-end" into page groups (1-based). Empty text means one group per page.
 * "7-" and "7-end" both run to the last page.
 */
export function parseRanges(text: string, pageCount: number): number[][] {
  const t = text.trim();
  if (!t) return Array.from({ length: pageCount }, (_, i) => [i + 1]);
  return t.split(',').map((part) => {
    const m = /^\s*(\d+)\s*(?:-\s*(\d*|end)\s*)?$/i.exec(part);
    if (!m) throw bad(`"${part.trim()}" isn't a page range`, 'Use ranges like 1-3, 5, 8- or 8-end.');
    const a = Number(m[1]);
    const rawEnd = m[2];
    const b = rawEnd === undefined ? a : (rawEnd === '' || rawEnd.toLowerCase() === 'end') ? pageCount : Number(rawEnd);
    if (a < 1 || b > pageCount || a > b) {
      throw bad(`pages ${part.trim()} don't exist in a ${pageCount}-page document`, `Use pages between 1 and ${pageCount}.`);
    }
    return Array.from({ length: b - a + 1 }, (_, i) => a + i);
  });
}

/** Parse a new page order such as "3, 1, 2". It must list every page exactly once. */
export function parseOrder(text: string, pageCount: number): number[] {
  const nums = text.split(/[\s,]+/).filter(Boolean).map(Number);
  if (nums.some((n) => !Number.isInteger(n) || n < 1 || n > pageCount)) {
    throw bad(`the new order has a page that doesn't exist`, `Use page numbers between 1 and ${pageCount}.`);
  }
  if (nums.length !== pageCount || new Set(nums).size !== pageCount) {
    throw bad(`the new order must list each of the ${pageCount} pages exactly once`, 'Check for missing or repeated pages.');
  }
  return nums;
}
