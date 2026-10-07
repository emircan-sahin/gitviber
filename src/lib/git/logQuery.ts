import type { LogFilter } from "../api";

/** A search with nothing in it: the plain history. */
const NO_FILTER: LogFilter = { grep: [], author: [], code: null, paths: [], follow: false };

const PREFIX = /^(author|path|code):(.*)$/is;
// An abbreviated SHA is 7 characters or more; shorter hex is too often a word ("added", "face").
const SHA = /^[0-9a-f]{7,40}$/i;
// A word, or one with quoted parts that keep its spaces.
const TOKEN = /(?:[^\s"]*"[^"]*"?)+[^\s"]*|\S+/g;

/**
 * History's search box: plain words must all be in the message; `author:`, `path:` and
 * `code:` (text a commit added or removed) narrow further. Quotes keep spaces in one term
 * (`author:"Ada Lovelace"`). `shas` are the words that could be a commit id.
 */
export function parseLogQuery(text: string): { filter: LogFilter; shas: string[] } {
  const filter: LogFilter = { ...NO_FILTER, grep: [], author: [], paths: [] };
  const shas: string[] = [];
  for (const [raw] of text.matchAll(TOKEN)) {
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

/**
 * The `author:` value that finds one author: git matches a fixed part of "Name <email>", so with
 * the email it is that person alone. A term can't hold a quote: then the longest stretch without
 * one, which is still a part of it.
 */
export function authorTerm(name: string, email?: string): string {
  const parts = (email ? `${name} <${email}>` : name).split('"').map((s) => s.trim());
  return parts.reduce((a, b) => (b.length > a.length ? b : a));
}

/** `text` narrowed to one author, as a click on a commit's author asks: the `author:` terms there were go, the rest stays. */
export function withAuthor(text: string, name: string, email?: string): string {
  const kept = [...text.matchAll(TOKEN)]
    .map(([t]) => t)
    .filter((t) => !/^author:/i.test(t.replaceAll('"', "")))
    // An unclosed quote would take in the author term after it.
    .map((t) => (t.split('"').length % 2 === 0 ? `${t}"` : t));
  const who = authorTerm(name, email);
  return [...kept, /\s/.test(who) ? `author:"${who}"` : `author:${who}`].join(" ");
}

export const isEmptyFilter = (f: LogFilter) => !f.grep.length && !f.author.length && !f.code && !f.paths.length;
