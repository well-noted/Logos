// Helpers for the experiment editor: NetLogo literals typed as one line of values, and the
// number of runs an experiment makes.

/** Splits a line of NetLogo literals into one string per value: numbers, `true`/`false`,
 * strings in double quotes (with backslash escapes) and lists in brackets. Returns an error
 * message for an unclosed string or list. */
export function splitLiterals(line: string): string[] | string {
  const out: string[] = [];
  let i = 0;
  while (i < line.length) {
    const c = line[i];
    if (/\s/.test(c)) { i++; continue; }
    const start = i;
    if (c === '"') {
      i++;
      while (i < line.length && line[i] !== '"') i += line[i] === "\\" ? 2 : 1;
      if (i >= line.length) return "A string is missing its closing quote.";
      i++;
    } else if (c === "[") {
      let depth = 0;
      for (; i < line.length; i++) {
        const ch = line[i];
        if (ch === '"') {
          i++;
          while (i < line.length && line[i] !== '"') i += line[i] === "\\" ? 2 : 1;
        } else if (ch === "[") depth++;
        else if (ch === "]" && --depth === 0) { i++; break; }
      }
      if (depth !== 0) return "A list is missing its closing bracket.";
    } else if (c === "]") {
      return "A closing bracket has no opening one.";
    } else {
      while (i < line.length && !/[\s[\]"]/.test(line[i])) i++;
    }
    out.push(line.slice(start, i));
  }
  return out;
}

/** The values joined back into one line. */
export function joinLiterals(values: string[]): string {
  return values.join(" ");
}

/** How many values a stepped value set makes, as BehaviorSpace counts them: first, first +
 * step, ... while not past last. Null when the numbers don't make a sequence. */
export function steppedCount(first: string, step: string, last: string): number | null {
  const f = Number(first), s = Number(step), l = Number(last);
  if (![f, s, l].every(Number.isFinite) || s === 0) return null;
  const n = Math.floor((l - f) / s + 1e-9) + 1;
  return n > 0 ? n : 0;
}
