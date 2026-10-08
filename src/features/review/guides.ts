import { useMemo } from "react";
import { api, errorMessage, github, type GuideAgent, type Guided, type GuideTarget, type RepoStatus, SUGGEST_CANCELLED } from "@/lib/api";
import { toast } from "@/lib/app/toast";
import { presetOf, programOf, reviewAgent, runDetails, withLeanFallback } from "@/lib/git/suggest";
import { type GuideSelection, type Selection, selectionPath } from "@/lib/repo/selection";
import { GUIDE_SCHEMA, guidePrompt } from "@/lib/review/guide";
import { RISKS_SCHEMA, risksPrompt } from "@/lib/review/risks";
import { getSettings } from "@/lib/settings";
import { isRecord, putRecent, readJson } from "@/lib/storage";
import { createStore } from "@/lib/store";
import { toReviewSettings, warnOldClaude } from "@/features/settings/SettingsDialog";

/** A run's answer as kept: as printed (parsed when shown), the range it read, who wrote it (with the model and effort its command named) and when. */
interface SavedRun extends Guided {
  program: string;
  /** Missing on guides from before it was kept. */
  details?: string[];
  at: number;
}

/** A guide as kept, with the sections marked done. */
interface SavedGuide extends SavedRun {
  done: number[];
}

const isRun = (v: unknown): v is SavedRun =>
  isRecord(v) &&
  typeof v.text === "string" &&
  typeof v.base === "string" &&
  typeof v.head === "string" &&
  typeof v.program === "string" &&
  typeof v.at === "number" &&
  (v.details === undefined || (Array.isArray(v.details) && v.details.every((d) => typeof d === "string")));

/** Where a guide, and its risks, are kept: per worktree, then per commit, per pull request, or per branch (`branch`: HEAD's, null detached) and base. */
export const guideId = (root: string, sel: GuideSelection, branch: string | null) =>
  sel.of === "commit" ? `${root}\0${sel.commit.sha}` : sel.of === "pull" ? `${root}\0${sel.pull.url}` : `${root}\0${branch ?? "HEAD"}\0${sel.base}`;

/**
 * Claude Code checks its answer against the run's schema; it and opencode are let read the patch
 * file. Others get the shape in the prompt: codex exec's --output-schema takes a file and OpenAI's
 * strict mode, which wants every field required, so it isn't used.
 */
function guideAgent(command: string, lean: boolean, schema: object): GuideAgent {
  const preset = presetOf(command);
  // --json-schema is as new as the lean flags: an older Claude Code goes without both.
  if (preset === "claude") return { args: lean ? ["--json-schema", JSON.stringify(schema)] : [], reads: "claude" };
  return { args: [], reads: preset === "opencode" ? "opencode" : null };
}

// A pull request's commits are fetched first when they're missing (pr_files), as its page does.
async function guideTarget(sel: GuideSelection): Promise<GuideTarget> {
  if (sel.of === "commit") return { of: "commit", sha: sel.commit.sha };
  if (sel.of === "branch") return { of: "branch", base: sel.base };
  const { base, head } = await github.files(sel.target, sel.pull);
  return { of: "pull", base, head, title: sel.pull.title };
}

/** A run on its way, stopped before it was written, or why it failed. */
export type Run = "running" | "stopped" | { error: string };

/** What a kind of run over a guide's range is: where it's kept, and what the agent is asked. */
interface RunKind<T extends SavedRun> {
  key: string;
  // localStorage is one quota for the whole app: one past maxSize lasts only until GitViber quits.
  max: number;
  maxSize: number;
  kind: "guide" | "risks";
  /** What it's called in toasts. */
  noun: string;
  schema: object;
  prompt: (language: string) => string;
  isSaved: (v: unknown) => v is T;
  /** A new answer as kept. */
  kept: (run: SavedRun) => T;
}

/**
 * Answers of one kind, kept by guide id, and their runs. A run isn't tied to the tab, so looking
 * at a file meanwhile doesn't stop it. The backend runs one of a kind at a time: starting one
 * stops one still running, and says so.
 */
function keptRuns<T extends SavedRun>(k: RunKind<T>) {
  // What storage couldn't take (full, turned off, too big) lasts until GitViber quits.
  const unsaved = new Map<string, T>();
  const saves = createStore(0);
  const runs = createStore<Record<string, Run>>({});
  // The latest run of each id: only it sets that id's state, so a stopped run's late reply
  // doesn't clear the one started after it.
  const latest = new Map<string, object>();
  let active: { sel: GuideSelection; token: object } | null = null;

  const load = (id: string) => {
    const v = unsaved.get(id) ?? readJson(k.key, {}, isRecord)[id];
    return k.isSaved(v) ? v : null;
  };

  const save = (id: string, value: T) => {
    const fits = value.text.length <= k.maxSize;
    // One too big to keep still takes the old one's place, which would come back after a restart.
    if (putRecent(k.key, id, fits ? value : null, k.max) && fits) unsaved.delete(id);
    else {
      const why = fits ? "App storage is full or turned off" : `It's over ${k.maxSize / 1024} KB`;
      if (!unsaved.has(id)) toast("error", `Could not keep the ${k.noun}`, `${why}: it lasts until GitViber quits.`);
      unsaved.set(id, value);
    }
    saves.set(saves.get() + 1);
  };

  const setRun = (id: string, run: Run | null) => {
    const { [id]: _, ...rest } = runs.get();
    runs.set(run ? { ...rest, [id]: run } : rest);
  };

  /** The answer kept under `id`, and its run. */
  const useKept = (id: string) => {
    const version = saves.use();
    const saved = useMemo(() => load(id), [id, version]);
    return { saved, run: runs.use()[id] ?? null };
  };

  /** Asks the Guided Review agent about `sel`, kept under `id` once it lands. */
  const generate = async (id: string, sel: GuideSelection) => {
    const settings = getSettings();
    const { command, models, efforts } = reviewAgent(settings);
    const prompt = k.prompt(settings.reviewLanguage);
    const program = programOf(command);
    // The line that answered, for the model and effort it names.
    let ran = "";
    const token = {};
    latest.set(id, token);
    const set = (run: Run | null) => latest.get(id) === token && setRun(id, run);
    const fail = (title: string, why: string) => {
      if (latest.get(id) !== token) return;
      setRun(id, { error: why });
      toast("error", title, why, toReviewSettings);
    };
    if (active) toast("info", `Stopped a ${k.noun}`, `${selectionPath(active.sel)} was still being written; one is written at a time.`);
    active = { sel, token };
    set("running");
    try {
      const target = await guideTarget(sel);
      // Cancelled, or another started, while the commits were fetched.
      if (active?.token !== token) throw SUGGEST_CANCELLED;
      const { value: guided, old } = await withLeanFallback(command, models, efforts, true, (line, lean) => {
        ran = line;
        // A second try started after another run would stop that one.
        return !lean && active?.token !== token ? Promise.reject(SUGGEST_CANCELLED) : api.suggestGuide(line, prompt, target, guideAgent(command, lean, k.schema), k.kind);
      });
      if (old) warnOldClaude();
      if (!guided.text.trim()) fail(`No ${k.noun} written`, `${program} printed nothing.`);
      else if (latest.get(id) === token) {
        save(id, k.kept({ ...guided, program, details: runDetails(ran), at: Date.now() }));
        set(null);
      }
    } catch (e) {
      if (e === SUGGEST_CANCELLED) set("stopped");
      else fail(`Couldn't write a ${k.noun}`, errorMessage(e));
    } finally {
      if (active?.token === token) active = null;
    }
  };

  /** Stops `id`'s run: one still fetching a pull request's commits has none in the backend to stop yet. */
  const cancel = (id: string) => {
    // Another tab's run, started since, goes on.
    if (active && latest.get(id) === active.token) active = null;
    return api.suggestCancel(k.kind).catch(() => {});
  };

  return { load, save, useKept, generate, cancel, running: (id: string) => runs.get()[id] === "running" };
}

const guides = keptRuns<SavedGuide>({
  key: "gitviber.guides",
  // A dozen guides of the usual 5-30 KB.
  max: 12,
  maxSize: 64 * 1024,
  kind: "guide",
  noun: "guided review",
  schema: GUIDE_SCHEMA,
  prompt: guidePrompt,
  isSaved: (v): v is SavedGuide => isRun(v) && "done" in v && Array.isArray(v.done) && v.done.every(Number.isInteger),
  kept: (run) => ({ ...run, done: [] }),
});

// Under the guide's own id, so a guide and its risks come and go together in the tab.
const risks = keptRuns<SavedRun>({
  key: "gitviber.guideRisks",
  max: 12,
  // At most eight risks of a few sentences: past this, it isn't the short list asked for.
  maxSize: 32 * 1024,
  kind: "risks",
  noun: "risk list",
  schema: RISKS_SCHEMA,
  prompt: risksPrompt,
  isSaved: isRun,
  kept: (run) => run,
});

export const useGuide = guides.useKept;
export const generateGuide = guides.generate;
export const cancelGuide = guides.cancel;

export const useRisks = risks.useKept;
export const findRisks = risks.generate;
export const cancelRisks = risks.cancel;

/** Opens `sel`'s tab, asking for its guide when there's none yet: the menu or button clicked is the ask. */
export function openGuide(sel: GuideSelection, status: RepoStatus | null, onOpen: (s: Selection, pin?: boolean) => void) {
  const id = guideId(status?.root ?? "", sel, status?.branch ?? null);
  if (!guides.load(id) && !guides.running(id)) void guides.generate(id, sel);
  onOpen(sel, true);
}

/** Marks section `i` of the guide under `id` done, or not. */
export function markDone(id: string, i: number, done: boolean) {
  const guide = guides.load(id);
  if (guide) guides.save(id, { ...guide, done: [...guide.done.filter((d) => d !== i), ...(done ? [i] : [])] });
}
