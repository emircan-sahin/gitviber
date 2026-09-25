import type { Selection } from "@/lib/repo/selection";

export interface Tab {
  key: string;
  sel: Selection;
  preview: boolean;
}

/** Which tabs Close Others / to the Left / to the Right / All close, around one. */
export type TabGroup = "others" | "left" | "right" | "all";

/** The keys of the tabs `which` names around the one at `i`. */
export const tabGroup = (tabs: Tab[], i: number, which: TabGroup) =>
  tabs.filter((_, j) => (which === "others" ? j !== i : which === "left" ? j < i : which === "right" ? j > i : true)).map((t) => t.key);
