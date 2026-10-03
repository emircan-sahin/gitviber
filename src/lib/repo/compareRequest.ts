import { createStore } from "../store";
import type { ComparePoint } from "./selection";

/** A request for the Compare screen, from somewhere that doesn't hold the tabs (the branch picker). A side left out is the screen's default. */
export interface CompareAsk {
  /** New per ask: the workspace opens each once. */
  id: number;
  base?: ComparePoint;
  head?: ComparePoint;
}

const asked = createStore<CompareAsk | null>(null);
let asks = 0;

export const askCompare = (sides: { base?: ComparePoint; head?: ComparePoint } = {}) => asked.set({ id: ++asks, ...sides });
export const useCompareAsk = asked.use;
