import { ask, message } from "@tauri-apps/plugin-dialog";
import { useSyncExternalStore } from "react";
import { api, errorMessage } from "../api";
import { toast } from "../app/toast";
import { basename } from "../path";
import { type FileEdit, loadEdits, saveEdits } from "../repo/session";
import { narrow } from "./indent";
import { createEditModel, detachModel, monaco, unitOf } from "./monaco";

/**
 * Files changed in the file view and not saved yet, by path. Each keeps the file's text from
 * when the edit began (`base`), so a save can tell whether something else changed the file since.
 * They're stored per worktree as well: macOS's ⌘Q quits without asking, and a reload or a repo
 * switch shouldn't lose them either. An edit restored from there has no model until it's shown.
 */
interface Edit {
  base: string;
  model: monaco.editor.ITextModel | null;
  /** The edited text while there's no model. */
  text: string;
}

let root: string | null = null;
const edits = new Map<string, Edit>();
let snapshot: ReadonlySet<string> = new Set();
const listeners = new Set<() => void>();

function changed() {
  snapshot = new Set(edits.keys());
  listeners.forEach((l) => l());
  persistSoon();
}

/** The paths with unsaved edits, for tabs and the file view. */
export const useEdited = () =>
  useSyncExternalStore(
    (l) => (listeners.add(l), () => listeners.delete(l)),
    () => snapshot,
  );

export const isEdited = (path: string) => edits.has(path);

/** Whether `model` holds an unsaved edit: a view done with it leaves it to the edit, undisposed. */
export const holdsEdit = (model: monaco.editor.ITextModel) => [...edits.values()].some((e) => e.model === model);

/** The file's text as edited in `model` (shown with widened indentation, see lib/editor/indent). */
const fileText = (model: monaco.editor.ITextModel) => narrow(model.getValue(monaco.editor.EndOfLinePreference.TextDefined, true), unitOf(model));

/** The unsaved text of `path`, if it has any. */
export function editedText(path: string) {
  const e = edits.get(path);
  return e && (e.model && !e.model.isDisposed() ? fileText(e.model) : e.text);
}

/** A model the file view shows editable: which file, the text saved there, and the version that is. */
const tracked = new WeakMap<monaco.editor.ITextModel, { path: string; base: string; clean: number }>();

/**
 * The model to show `path` with while it has an unsaved edit (made from the stored text after a
 * restart), else the model on show when it's this file's and still holds `disk`, as after a save.
 */
export function editModel(path: string, lang: string, disk: string, shown: monaco.editor.ITextModel | null) {
  const e = edits.get(path);
  if (e) {
    if (!e.model || e.model.isDisposed()) {
      e.model = createEditModel(e.text, lang, path);
      // Not the file's text: dirty until saved, even undone to what it was loaded with.
      tracked.set(e.model, { path, base: e.base, clean: -1 });
      e.model.onDidChangeContent(() => onChange(e.model!));
    }
    return e.model;
  }
  const t = shown && tracked.get(shown);
  if (!shown || !t || t.path !== path || fileText(shown) !== disk) return null;
  t.base = disk;
  return shown;
}

/** Follows the edits made to `model`, the file view's model of `path` as it is on disk (`base`). */
export function track(model: monaco.editor.ITextModel, path: string, base: string) {
  if (tracked.has(model)) return;
  tracked.set(model, { path, base, clean: model.getAlternativeVersionId() });
  model.onDidChangeContent(() => onChange(model));
}

function onChange(model: monaco.editor.ITextModel) {
  const t = tracked.get(model)!;
  // Its text is no longer what the model cache keeps it under.
  detachModel(model);
  const e = edits.get(t.path);
  // Undone back to the saved text.
  if (model.getAlternativeVersionId() === t.clean) {
    if (e?.model === model) edits.delete(t.path);
    return changed();
  }
  if (e?.model === model) return persistSoon();
  edits.set(t.path, { base: t.base, model, text: "" });
  changed();
}

/** Saves `path`'s edit. False when it wasn't: the write failed, or the file changed on disk and overwriting it was declined. */
export function saveEdit(path: string): Promise<boolean> {
  // A second ⌘S while the first writes would find the file changed on disk, by the first.
  let run = saving.get(path);
  if (!run) {
    run = write(path).finally(() => saving.delete(path));
    saving.set(path, run);
  }
  return run;
}
const saving = new Map<string, Promise<boolean>>();

async function write(path: string) {
  const e = edits.get(path);
  if (!e) return true;
  const model = e.model && !e.model.isDisposed() ? e.model : null;
  // Undo then stops at what was saved, as VS Code's does, instead of taking back the whole run of typing.
  model?.pushStackElement();
  const text = model ? fileText(model) : e.text;
  const version = model?.getAlternativeVersionId();
  try {
    const disk = await api.readFile(path);
    if (disk.exists && !disk.lossy && disk.text !== e.base) {
      const ok = await ask(`${basename(path)} changed on disk since you began editing it. Overwrite it with your version?`, {
        title: "Save",
        kind: "warning",
        okLabel: "Overwrite",
        cancelLabel: "Cancel",
      });
      if (!ok) return false;
    }
    await api.writeFile(path, text);
  } catch (err) {
    toast("error", `Could not save ${basename(path)}`, errorMessage(err));
    return false;
  }
  // Typing may have gone on while it saved: that stays an edit, of the text now on disk.
  const now = edits.get(path);
  if (now !== e) return true;
  const t = model && tracked.get(model);
  if (t) Object.assign(t, { base: text, clean: version! });
  if (model && model.getAlternativeVersionId() !== version) e.base = text;
  else edits.delete(path);
  changed();
  return true;
}

/** Drops `path`'s edit; its model goes too unless a view shows it (which then releases it). */
function discard(path: string) {
  const e = edits.get(path);
  if (!e) return;
  edits.delete(path);
  if (e.model && !e.model.isDisposed() && !e.model.isAttachedToEditor()) e.model.dispose();
}

/**
 * Asks what to do with the unsaved edits of `paths` before their tabs close: save them, drop them,
 * or cancel. True when the tabs may close.
 */
export async function settleEdits(paths: string[]): Promise<boolean> {
  const dirty = paths.filter((p) => edits.has(p));
  if (!dirty.length) return true;
  const answer = await message(
    dirty.length === 1 ? `Do you want to save the changes you made to ${basename(dirty[0])}?` : `Do you want to save the changes you made to ${dirty.length} files?`,
    { title: "Unsaved changes", kind: "warning", buttons: { yes: "Save", no: "Don't Save", cancel: "Cancel" } },
  );
  if (answer === "Save" || answer === "Yes") {
    for (const p of dirty) if (!(await saveEdit(p))) return false;
    return true;
  }
  if (answer !== "Don't Save" && answer !== "No") return false;
  dirty.forEach(discard);
  changed();
  return true;
}

/** Explorer renamed or trashed `from` (a file or a folder): its edits follow, or go with it (`to` null). */
export function moveEdits(from: string, to: string | null) {
  const hit = [...edits.keys()].filter((p) => p === from || p.startsWith(`${from}/`));
  if (!hit.length) return;
  for (const p of hit) {
    if (to === null) {
      discard(p);
      continue;
    }
    const e = edits.get(p)!;
    const next = to + p.slice(from.length);
    edits.delete(p);
    edits.set(next, e);
    const t = e.model && tracked.get(e.model);
    if (t) t.path = next;
  }
  changed();
}

/** The worktree now open (null: none): the last one's edits are stored and let go, this one's come back. */
export function openEdits(next: string | null) {
  if (next === root) return;
  persist();
  for (const e of edits.values()) if (e.model && !e.model.isAttachedToEditor()) e.model.dispose();
  edits.clear();
  root = next;
  if (next) for (const [path, e] of Object.entries(loadEdits(next))) edits.set(path, { base: e.base, model: null, text: e.text });
  // Called while the new workspace renders, before anything in it subscribes: no one to tell.
  snapshot = new Set(edits.keys());
}

let timer: ReturnType<typeof setTimeout> | undefined;
function persistSoon() {
  clearTimeout(timer);
  timer = setTimeout(persist, 300);
}

function persist() {
  clearTimeout(timer);
  if (!root) return;
  const out: Record<string, FileEdit> = {};
  for (const [path, e] of edits) out[path] = { text: editedText(path)!, base: e.base };
  saveEdits(root, out);
}

// A reload or a quit may land inside the wait.
window.addEventListener("pagehide", persist);
