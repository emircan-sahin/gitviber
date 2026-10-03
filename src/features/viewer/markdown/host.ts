import { createContext, type ReactNode } from "react";

/** What rendered markdown asks of the place it's shown in: an Obsidian vault resolves embeds and wikilinks. */
export interface MarkdownHost {
  /** An ![[embed]], alone on its line (`block`) or inline. */
  embed?: (target: string, alias: string, block: boolean) => ReactNode;
  /** A [[wikilink]] in a property value. */
  wikilink?: (target: string, label: string) => ReactNode;
}

export const MarkdownHostContext = createContext<MarkdownHost>({});
