/**
 * Fuzzy matching for the command palette and quick open: the query's characters in order,
 * scored as fzy does, so matches at word starts and in a row rank first. Case and spaces in the
 * query are ignored ("open file", "gtp" → "Git: Push" both match).
 */

export interface Match {
  score: number;
  /** Indices into the text of the matched characters, for highlighting. */
  hits: number[];
}

const GAP_LEADING = -0.005;
const GAP_TRAILING = -0.005;
const GAP_INNER = -0.01;
const CONSECUTIVE = 1;

function bonus(text: string, j: number): number {
  if (j === 0) return 0.9;
  const prev = text[j - 1];
  if (prev === "/" || prev === "\\") return 0.9;
  if (prev === "-" || prev === "_" || prev === " " || prev === ":") return 0.8;
  if (prev === ".") return 0.6;
  const c = text[j];
  return prev === prev.toLowerCase() && c !== c.toLowerCase() ? 0.7 : 0;
}

/** Scores `text` against a query from `prepareQuery`; null when not every character is in it, in order. */
export function fuzzyMatch(query: string, text: string): Match | null {
  const m = query.length;
  const n = text.length;
  if (!m) return { score: 0, hits: [] };
  if (m > n) return null;
  let lower = text.toLowerCase();
  // A few characters lower-case to two ("İ" → "i̇"), which would shift every index after them.
  if (lower.length !== n) lower = Array.from(text, (c) => c.toLowerCase().slice(0, c.length)).join("");
  // Most texts fail here, before the O(m·n) scoring.
  for (let i = 0, j = 0; i < m; i++, j++) {
    j = lower.indexOf(query[i], j);
    if (j < 0) return null;
  }
  if (m === n) return { score: Infinity, hits: [...Array(n).keys()] };

  // D: best score with query[i] matched at text[j]; M: best score for query[..i] within text[..j].
  const D = Array.from({ length: m }, () => new Float64Array(n));
  const M = Array.from({ length: m }, () => new Float64Array(n));
  const bonuses = Array.from({ length: n }, (_, j) => bonus(text, j));
  for (let i = 0; i < m; i++) {
    let prev = -Infinity;
    const gap = i === m - 1 ? GAP_TRAILING : GAP_INNER;
    for (let j = 0; j < n; j++) {
      if (lower[j] === query[i]) {
        let score = -Infinity;
        if (i === 0) score = j * GAP_LEADING + bonuses[j];
        else if (j > 0) score = Math.max(M[i - 1][j - 1] + bonuses[j], D[i - 1][j - 1] + CONSECUTIVE);
        D[i][j] = score;
        M[i][j] = prev = Math.max(score, prev + gap);
      } else {
        D[i][j] = -Infinity;
        M[i][j] = prev = prev + gap;
      }
    }
  }

  const hits = new Array<number>(m);
  let required = false;
  for (let i = m - 1, j = n - 1; i >= 0; i--) {
    for (; j >= 0; j--) {
      if (D[i][j] !== -Infinity && (required || D[i][j] === M[i][j])) {
        required = i > 0 && j > 0 && M[i][j] === D[i - 1][j - 1] + CONSECUTIVE;
        hits[i] = j--;
        break;
      }
    }
  }
  return { score: M[m - 1][n - 1], hits };
}

export const prepareQuery = (q: string) => q.toLowerCase().replace(/\s+/g, "");

/**
 * A path, matched on its file name first, as VS Code does: "fuzzy" finds src/lib/fuzzy.ts before
 * src/fuzzy-old/index.ts. Falls back to the whole path, so "lib/fuz" works too.
 */
export function matchPath(query: string, path: string): Match | null {
  const start = path.lastIndexOf("/") + 1;
  const name = fuzzyMatch(query, path.slice(start));
  if (name) return { score: name.score + 10, hits: name.hits.map((h) => h + start) };
  return fuzzyMatch(query, path);
}
