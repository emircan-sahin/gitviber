/**
 * What a find matches, the same for every find box that isn't Monaco's (a page, the terminal,
 * search in files) and with Monaco's three toggles. No DOM, so it runs under node:test.
 */

export interface FindOptions {
  matchCase: boolean;
  wholeWord: boolean;
  regex: boolean;
}

export const NO_OPTIONS: FindOptions = { matchCase: false, wholeWord: false, regex: false };

/** The query as a global regex, or the reason it isn't one (a regex that doesn't parse). */
export function compileFind(query: string, o: FindOptions): RegExp | Error {
  const source = o.regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  try {
    // Not "u", as Monaco: it would refuse everyday escapes such as [\w-].
    return new RegExp(source, o.matchCase ? "g" : "gi");
  } catch (e) {
    return e instanceof Error ? e : new Error(String(e));
  }
}

// git grep -w's word characters.
const WORD = /[\p{L}\p{N}_]/u;
const wordAt = (text: string, i: number) => i >= 0 && i < text.length && WORD.test(text[i]);

/**
 * Where `query` matches in `text`: [start, end) pairs, empty matches left out. Whole word
 * wants no word character on either side, trying later starts when one doesn't fit, as git grep -w does.
 */
export function findMatches(text: string, query: string, o: FindOptions, limit = 10_000): [number, number][] | Error {
  if (!query) return [];
  const re = compileFind(query, o);
  if (re instanceof Error) return re;
  const out: [number, number][] = [];
  for (let m = re.exec(text); m && out.length < limit; m = re.exec(text)) {
    const [start, end] = [m.index, m.index + m[0].length];
    if (end === start) {
      re.lastIndex = start + 1;
      continue;
    }
    if (o.wholeWord && (wordAt(text, start - 1) || wordAt(text, end))) {
      re.lastIndex = start + 1;
      continue;
    }
    out.push([start, end]);
  }
  return out;
}

/** The key that flips an option in a find box, Monaco's: ⌥⌘C / ⌥⌘W / ⌥⌘R on macOS, Alt+C / Alt+W / Alt+R elsewhere. */
export function optionKey(e: { code: string; altKey: boolean; metaKey: boolean; ctrlKey: boolean; shiftKey: boolean }, mac: boolean): keyof FindOptions | null {
  if (!e.altKey || e.shiftKey || e.ctrlKey || e.metaKey !== mac) return null;
  return ({ KeyC: "matchCase", KeyW: "wholeWord", KeyR: "regex" } as const)[e.code as "KeyC"] ?? null;
}
