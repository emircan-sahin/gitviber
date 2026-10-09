import { createStore } from "../store";

/** A page to open in a browser tab, from somewhere that doesn't hold the tabs (a terminal pane's menu). */
export interface PageAsk {
  /** New per ask: the workspace opens each once. */
  id: number;
  url: string;
}

const asked = createStore<PageAsk | null>(null);
let asks = 0;

export const askOpenPage = (url: string) => asked.set({ id: ++asks, url });
export const usePageAsk = asked.use;
