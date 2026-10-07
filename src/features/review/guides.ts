import { useMemo } from "react";
import { api, errorMessage, type GuideAgent, type Guided, type GuideTarget, type RepoStatus, SUGGEST_CANCELLED } from "@/lib/api";
import { toast } from "@/lib/app/toast";
import { commandLine, presetOf, programOf, runDetails } from "@/lib/git/suggest";
import { type GuideSelection, type Selection, selectionPath } from "@/lib/repo/selection";
import { GUIDE_PROMPT, GUIDE_SCHEMA } from "@/lib/review/guide";
import { getSettings } from "@/lib/settings";
import { isRecord, putRecent, readJson } from "@/lib/storage";
import { createStore } from "@/lib/store";
import { toSuggestSettings } from "@/features/settings/SettingsDialog";

const KEY = "gitviber.guides";
// localStorage is one quota for the whole app: a dozen guides of the usual 5-30 KB, and one
// past MAX_SIZE lasts only until GitViber quits.
const MAX = 12;
const MAX_SIZE = 64 * 1024;

/** A guide as kept: the answer as printed (parsed when shown), the range it read, who wrote it (with the model and effort its command named) and when, the sections marked done. */
interface SavedGuide extends Guided {
  program: string;
  /** Missing on guides from before it was kept. */
  details?: string[];
  at: number;
  done: number[];
}

const isSaved = (v: unknown): v is SavedGuide =>
  isRecord(v) &&
  typeof v.text === "string" &&
  typeof v.base === "string" &&
  typeof v.head === "string" &&
  typeof v.program === "string" &&
  typeof v.at === "number" &&
  Array.isArray(v.done) &&
  v.done.every(Number.isInteger) &&
  (v.details === undefined || (Array.isArray(v.details) && v.details.every((d) => typeof d === "string")));

/** Where a guide is kept: per worktree, then per commit, or per branch (`branch`: HEAD's, null detached) and base. */
export const guideId = (root: string, sel: GuideSelection, branch: string | null) => (sel.of === "commit" ? `${root}\0${sel.commit.sha}` : `${root}\0${branch ?? "HEAD"}\0${sel.base}`);

/**
 * Claude Code checks its answer against the guide's schema; it and opencode are let read the patch
 * file. Others get the shape in the prompt: codex exec's --output-schema takes a file and OpenAI's
 * strict mode, which wants every field required, so it isn't used.
 */
function guideAgent(command: string): GuideAgent {
  const preset = presetOf(command);
  if (preset === "claude") return { args: ["--json-schema", JSON.stringify(GUIDE_SCHEMA)], reads: "claude" };
  return { args: [], reads: preset === "opencode" ? "opencode" : null };
}

const guideTarget = (sel: GuideSelection): GuideTarget => (sel.of === "commit" ? { of: "commit", sha: sel.commit.sha } : { of: "branch", base: sel.base });

// What storage couldn't take (full, turned off, too big) lasts until GitViber quits.
const unsaved = new Map<string, SavedGuide>();
const saves = createStore(0);

function load(id: string) {
  const v = unsaved.get(id) ?? readJson(KEY, {}, isRecord)[id];
  return isSaved(v) ? v : null;
}

function save(id: string, guide: SavedGuide) {
  const fits = guide.text.length <= MAX_SIZE;
  // One too big to keep still takes the old one's place, which would come back after a restart.
  if (putRecent(KEY, id, fits ? guide : null, MAX) && fits) unsaved.delete(id);
  else {
    const why = fits ? "App storage is full or turned off" : `It's over ${MAX_SIZE / 1024} KB`;
    if (!unsaved.has(id)) toast("error", "Could not keep the guided review", `${why}: it lasts until GitViber quits.`);
    unsaved.set(id, guide);
  }
  saves.set(saves.get() + 1);
}

/** A guide on its way, stopped before it was written, or why it failed; by guide id. */
type Run = "running" | "stopped" | { error: string };
const runs = createStore<Record<string, Run>>({});
// The latest run of each id: only it sets that id's state, so a stopped run's late reply
// doesn't clear the one started after it.
const latest = new Map<string, object>();
// The one running: the backend runs one guide at a time.
let active: { sel: GuideSelection; token: object } | null = null;

function setRun(id: string, run: Run | null) {
  const { [id]: _, ...rest } = runs.get();
  runs.set(run ? { ...rest, [id]: run } : rest);
}

/** The guide kept under `id`, and its run. */
export function useGuide(id: string) {
  const version = saves.use();
  const saved = useMemo(() => load(id), [id, version]);
  return { saved, run: runs.use()[id] ?? null };
}

/**
 * Asks the configured agent CLI for a guide of `sel`, kept under `id` once it lands. Not tied
 * to the tab, so looking at a file meanwhile doesn't stop it. The backend runs one at a time:
 * starting this one stops one still running, and says so.
 */
export async function generateGuide(id: string, sel: GuideSelection) {
  const { suggestCommand, suggestModels, suggestEfforts } = getSettings();
  const program = programOf(suggestCommand);
  const line = commandLine(suggestCommand, suggestModels, suggestEfforts, true);
  const token = {};
  latest.set(id, token);
  const set = (run: Run | null) => latest.get(id) === token && setRun(id, run);
  const fail = (title: string, why: string) => {
    if (latest.get(id) !== token) return;
    setRun(id, { error: why });
    toast("error", title, why, toSuggestSettings);
  };
  if (active) toast("info", "Stopped a guided review", `${selectionPath(active.sel)} was still being written; one is written at a time.`);
  active = { sel, token };
  set("running");
  try {
    const guided = await api.suggestGuide(line, GUIDE_PROMPT, guideTarget(sel), guideAgent(suggestCommand));
    if (!guided.text.trim()) fail("No guided review written", `${program} printed nothing.`);
    else if (latest.get(id) === token) {
      save(id, { ...guided, program, details: runDetails(line), at: Date.now(), done: [] });
      set(null);
    }
  } catch (e) {
    if (e === SUGGEST_CANCELLED) set("stopped");
    else fail("Couldn't write a guided review", errorMessage(e));
  } finally {
    if (active?.token === token) active = null;
  }
}

/** Opens `sel`'s tab, asking for its guide when there's none yet: the menu or button clicked is the ask. */
export function openGuide(sel: GuideSelection, status: RepoStatus | null, onOpen: (s: Selection, pin?: boolean) => void) {
  const id = guideId(status?.root ?? "", sel, status?.branch ?? null);
  if (!load(id) && runs.get()[id] !== "running") void generateGuide(id, sel);
  onOpen(sel, true);
}

export const cancelGuide = () => api.suggestCancel("guide").catch(() => {});

/** Marks section `i` of the guide under `id` done, or not. */
export function markDone(id: string, i: number, done: boolean) {
  const guide = load(id);
  if (guide) save(id, { ...guide, done: [...guide.done.filter((d) => d !== i), ...(done ? [i] : [])] });
}
