import { useMemo } from "react";
import { api, errorMessage, type Guided, type GuideTarget, type RepoStatus, SUGGEST_CANCELLED } from "@/lib/api";
import { toast } from "@/lib/app/toast";
import { commandLine, programOf } from "@/lib/git/suggest";
import type { GuideSelection, Selection } from "@/lib/repo/selection";
import { GUIDE_PROMPT } from "@/lib/review/guide";
import { getSettings } from "@/lib/settings";
import { isRecord, putRecent, readJson } from "@/lib/storage";
import { createStore } from "@/lib/store";
import { openSettings } from "@/features/settings/SettingsDialog";

const KEY = "gitviber.guides";
// Each is the agent's answer, a few to tens of KB: reopening one doesn't run the agent again.
const MAX = 20;

/** A guide as kept: the answer as printed (parsed when shown), the range it read, who wrote it and when, the sections marked done. */
export interface SavedGuide extends Guided {
  program: string;
  at: number;
  done: number[];
}

const isSaved = (v: unknown): v is SavedGuide =>
  isRecord(v) && typeof v.text === "string" && typeof v.base === "string" && typeof v.head === "string" && typeof v.program === "string" && typeof v.at === "number" && Array.isArray(v.done);

/** Where a guide is kept: per worktree, then per commit, or per branch (`branch`: HEAD's, null detached) and base. */
export const guideId = (root: string, sel: GuideSelection, branch: string | null) => (sel.of === "commit" ? `${root}\0${sel.commit.sha}` : `${root}\0${branch ?? "HEAD"}\0${sel.base}`);

export const guideTarget = (sel: GuideSelection): GuideTarget => (sel.of === "commit" ? { of: "commit", sha: sel.commit.sha } : { of: "branch", base: sel.base });

// What storage couldn't take (full, turned off) lasts until GitViber quits.
const unsaved = new Map<string, SavedGuide>();
const saves = createStore(0);

function load(id: string) {
  const v = unsaved.get(id) ?? readJson(KEY, {}, isRecord)[id];
  return isSaved(v) ? v : null;
}

function save(id: string, guide: SavedGuide) {
  if (putRecent(KEY, id, guide, MAX)) unsaved.delete(id);
  else {
    if (!unsaved.size) toast("error", "Could not save the guided review", "App storage is full or turned off: it lasts until GitViber quits.");
    unsaved.set(id, guide);
  }
  saves.set(saves.get() + 1);
}

/** A guide on its way, or why the last one failed; by guide id. */
type Run = "running" | { error: string };
const runs = createStore<Record<string, Run>>({});
const setRun = (id: string, run: Run | null) => {
  const { [id]: _, ...rest } = runs.get();
  runs.set(run ? { ...rest, [id]: run } : rest);
};

/** The guide kept under `id`, and its run. */
export function useGuide(id: string) {
  const version = saves.use();
  const saved = useMemo(() => load(id), [id, version]);
  return { saved, run: runs.use()[id] ?? null };
}

/**
 * Asks the configured agent CLI for a guide of `target`, kept under `id` once it lands. Not
 * tied to the tab, so looking at a file meanwhile doesn't stop it; one runs at a time, and
 * starting another stops it.
 */
export async function generateGuide(id: string, target: GuideTarget) {
  const { suggestCommand, suggestModels } = getSettings();
  const program = programOf(suggestCommand);
  const fail = (title: string, why: string) => {
    setRun(id, { error: why });
    // A missing CLI or a stale model id is fixed there.
    toast("error", title, why, { label: "Open Settings", run: () => openSettings("commit") });
  };
  setRun(id, "running");
  try {
    const guided = await api.suggestGuide(commandLine(suggestCommand, suggestModels), GUIDE_PROMPT, target);
    if (!guided.text.trim()) return fail("No guided review written", `${program} printed nothing.`);
    save(id, { ...guided, program, at: Date.now(), done: [] });
    setRun(id, null);
  } catch (e) {
    if (e === SUGGEST_CANCELLED) setRun(id, null);
    else fail("Couldn't write a guided review", errorMessage(e));
  }
}

/** Opens `sel`'s tab, asking for its guide when there's none yet: the menu or button clicked is the ask. */
export function openGuide(sel: GuideSelection, status: RepoStatus | null, onOpen: (s: Selection, pin?: boolean) => void) {
  const id = guideId(status?.root ?? "", sel, status?.branch ?? null);
  if (!load(id) && runs.get()[id] !== "running") void generateGuide(id, guideTarget(sel));
  onOpen(sel, true);
}

export const cancelGuide = () => api.suggestCancel("guide").catch(() => {});

/** Marks section `i` of the guide under `id` done, or not. */
export function markDone(id: string, i: number, done: boolean) {
  const guide = load(id);
  if (guide) save(id, { ...guide, done: [...guide.done.filter((d) => d !== i), ...(done ? [i] : [])] });
}
