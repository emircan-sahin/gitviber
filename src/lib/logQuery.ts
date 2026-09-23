import type { LogFilter } from "./api";

/** A search with nothing in it: the plain history. */
export const NO_FILTER: LogFilter = { grep: [], author: [], code: null, paths: [], follow: false };

const PREFIX = /^(author|path|code):(.*)$/is;
// An abbreviated SHA is 7 characters or more; shorter hex is too often a word ("added", "face").
const SHA = /^[0-9a-f]{7,40}$/i;

/**
 * History's search box: plain words must all be in the message; `author:`, `path:` and
 * `code:` (text a commit added or removed) narrow further. Quotes keep spaces in one term
 * (`author:"Ada Lovelace"`). `shas` are the words that could be a commit id.
 */
export function parseLogQuery(text: string): { filter: LogFilter; shas: string[] } {
  const filter: LogFilter = { ...NO_FILTER, grep: [], author: [], paths: [] };
  const shas: string[] = [];
  for (const [raw] of text.matchAll(/(?:[^\s"]*"[^"]*"?)+[^\s"]*|\S+/g)) {
    const token = raw.replaceAll('"', "");
    const m = PREFIX.exec(token);
    if (!m) {
      if (!token) continue;
      filter.grep.push(token);
      if (SHA.test(token)) shas.push(token);
      continue;
    }
    const value = m[2];
    if (!value) continue;
    const key = m[1].toLowerCase();
    if (key === "author") filter.author.push(value);
    else if (key === "code") filter.code = value;
    else filter.paths.push(value.replace(/^\.\//, "").replace(/\/+$/, ""));
  }
  return { filter, shas };
}

export const isEmptyFilter = (f: LogFilter) => !f.grep.length && !f.author.length && !f.code && !f.paths.length;
