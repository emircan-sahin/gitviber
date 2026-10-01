import { useEffect, useSyncExternalStore } from "react";
import { api } from "../api";
import { toast } from "../app/toast";
import { loadNotes, saveNotes } from "../repo/session";
import { checkNotes, isLive, type ReviewNote } from "./notes";

// The open worktree's review notes, outside React: the code view's Monaco zones read them too.
let root: string | null = null;
let notes: ReviewNote[] = [];
// Each noted file's lines as last read, to tell how far edits above a note moved it.
const seen = new Map<string, string[]>();
const listeners = new Set<() => void>();

/** The notes of `worktree`, the one the window shows from now on. */
export function openNotes(worktree: string) {
  if (worktree === root) return;
  root = worktree;
  seen.clear();
  // Called while the new workspace renders, before anything in it subscribes: no one to tell.
  notes = loadNotes(worktree);
}

// Said once until a save works again, not on every note.
let unsaved = false;

function set(next: ReviewNote[]) {
  if (next === notes) return;
  notes = next;
  listeners.forEach((l) => l());
  if (!root) return;
  const saved = saveNotes(root, next);
  if (!saved && !unsaved) toast("error", "Could not save the review notes", "App storage is full or turned off: they last until GitViber quits.");
  unsaved = !saved;
}

export const getNotes = () => notes;
export function subscribeNotes(l: () => void) {
  listeners.add(l);
  return () => void listeners.delete(l);
}
export const useNotes = () => useSyncExternalStore(subscribeNotes, getNotes);
/** The worktree the notes are of, for sending them to its terminal. */
export const notesRoot = () => root;

export function addNote(note: Omit<ReviewNote, "id">) {
  set([...notes, { ...note, id: crypto.randomUUID() }]);
}

export function updateNote(id: string, patch: Partial<Pick<ReviewNote, "resolved">>) {
  set(notes.map((n) => (n.id === id ? { ...n, ...patch } : n)));
}

export function removeNote(id: string) {
  set(notes.filter((n) => n.id !== id));
}

export function clearResolved() {
  set(notes.filter((n) => !n.resolved));
}

/**
 * Keeps the notes on the files on disk up with them: on every change there (`revision`), each
 * noted file is read again, and its notes move with their lines or turn outdated.
 */
export function useNoteCheck(revision: number) {
  const paths = [...new Set(useNotes().flatMap((n) => (isLive(n) && !n.resolved ? [n.path] : [])))].join("\0");
  useEffect(() => {
    if (!paths) return;
    let alive = true;
    for (const path of paths.split("\0"))
      api
        .readFile(path)
        .then((f) => {
          if (!alive) return;
          const lines = f.exists && !f.binary && !f.tooLarge ? f.text.split(/\r?\n/) : null;
          set(checkNotes(notes, path, lines, seen.get(path)));
          if (lines) seen.set(path, lines);
          else seen.delete(path);
        })
        .catch(() => {});
    return () => {
      alive = false;
    };
  }, [paths, revision]);
}
