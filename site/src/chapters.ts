import type { Messages } from "./i18n/locales.ts";

/** The story's chapters in order, the way the page, its structured data and llms-full.txt list them. */
export const chapters = (t: Messages) => [...t.agents.steps, t.review, t.terminal, t.explorer, t.commit];

/** The shortcut a chapter's text names at {keys}. */
export const CHAPTER_KEYS: Record<number, string[]> = { 5: ["⌘", "J"], 6: ["⌘", "P"] };

/** A chapter's text with its shortcut spelled out, for plain-text readers. */
export const chapterText = (text: string, i: number) => text.replace("{keys}", (CHAPTER_KEYS[i] ?? []).join(""));
