// Review notes in the code view, drawn as a PR's line comments are (ReviewThreads): each under the
// lines it's on, wherever the file on show has those lines, and "Add Review Note…" in the context
// menu, or the Add Review Note key, for a new one on the selected lines.
import { Check, MessageSquareText, Trash2 } from "lucide-react";
import type { DiffPair } from "@/lib/api";
import type { monaco } from "@/lib/editor/monaco";
import { selectedLines } from "@/lib/editor/lineActions";
import { type Anchor, anchorAt, findNote, noteLines, placeNotes, type ReviewNote } from "@/lib/review/notes";
import { addNote, getNotes, removeNote, subscribeNotes, updateNote } from "@/lib/review/noteStore";
import { Composer } from "./Composer";
import { newLineBefore, zoneWidget } from "./zones";
import { codeEditor, type Editor, isDiff } from "@/features/viewer/activeEditor";

/** The file on show, as notes are placed in it and written on it. */
export interface NotesShown {
  /** Each side's path: a rename's old side is under its old name. */
  path: string;
  oldPath: string;
  pair: DiffPair;
  unified: boolean;
  /** A commit's version, for the prompt ("commit 1a2b3c4"); none: the files on disk (or the index). */
  at?: string;
}

type Side = "original" | "modified";

const linesOf = (s: NotesShown, side: Side) => {
  const t = side === "original" ? s.pair.original : s.pair.modified;
  return t.exists ? t.text.split(/\r?\n/) : null;
};

/** The notes in `e` (a diff, or a file's own view) for what `get()` shows; `update` redraws them. */
export function followReviewNotes(e: Editor, get: () => NotesShown | null) {
  const editor = (side: Side) => (side === "original" && isDiff(e) ? e.getOriginalEditor() : codeEditor(e));
  let zones: (() => void)[] = [];
  // A new note being written: its lines as they were picked (it follows them as the file changes),
  // and what's typed so far.
  let draft: { path: string; side: Side; anchor: Anchor; body: string } | null = null;
  // Its box, kept while notes come and go (an agent's edits recheck them) so typing goes on undisturbed;
  // drawn again only when its file's text or its place changes. `where`: the model and line it's under.
  let draftZone: { where: string; model: monaco.editor.ITextModel | null; dispose: () => void } | null = null;

  // Unified view draws the old side among the new one's lines.
  const place = (s: NotesShown, side: Side, line: number, content: React.ReactNode) =>
    side === "original" && s.unified ? zoneWidget(codeEditor(e), newLineBefore(s.pair.rows, line), content) : zoneWidget(editor(side), line, content);

  const clear = () => {
    zones.forEach((dispose) => dispose());
    zones = [];
  };
  const dropDraftZone = () => {
    draftZone?.dispose();
    draftZone = null;
  };

  const update = () => {
    clear();
    const s = get();
    if (!s) return dropDraftZone();
    const lines = { original: isDiff(e) ? linesOf(s, "original") : null, modified: linesOf(s, "modified") };
    for (const p of placeNotes(getNotes(), { path: s.path, oldPath: s.oldPath, oldLines: lines.original, newLines: lines.modified }))
      zones.push(place(s, p.old ? "original" : "modified", p.end, <NoteCard note={p.note} />));
    if (draft?.path !== s.path) draft = null;
    if (!draft) return dropDraftZone();
    const d = draft;
    // Its lines where the file has them now; where they were, if they're gone.
    const start = findNote(lines[d.side] ?? [], d.anchor) ?? d.anchor.start;
    d.anchor = { ...d.anchor, start, end: start + d.anchor.end - d.anchor.start };
    const { end } = d.anchor;
    const model = editor(d.side).getModel();
    const where = `${d.side}:${end}:${s.unified}`;
    if (draftZone?.where === where && draftZone.model === model) return;
    dropDraftZone();
    const cancel = () => {
      draft = null;
      update();
    };
    const dispose = place(
      s,
      d.side,
      end,
      <Composer
        label={`Note on ${noteLines(d.anchor)}${d.side === "original" ? " (old)" : ""}`}
        initial={d.body}
        onDraft={(body) => (d.body = body)}
        onCancel={cancel}
        onSubmit={async (body) => {
          draft = null;
          addNote({ ...d.anchor, path: d.side === "original" ? s.oldPath : s.path, body: body.trim(), old: d.side === "original" || undefined, at: s.at });
        }}
      />,
    );
    draftZone = { where, model, dispose };
  };

  /** A note on `side`'s selected lines, else the cursor's line. */
  const begin = (side: Side) => {
    const s = get();
    const text = s && linesOf(s, side);
    const sel = editor(side).getSelection();
    if (!s || !text || !sel) return;
    const [start, end] = selectedLines(sel).map((l) => Math.min(l, text.length));
    draft = { path: s.path, side, anchor: anchorAt(text, start, end), body: "" };
    dropDraftZone();
    update();
  };

  const subs: monaco.IDisposable[] = [];
  for (const side of isDiff(e) ? (["original", "modified"] as const) : (["modified"] as const)) {
    const code = editor(side);
    const has = code.createContextKey<boolean>("gitviberNotes", false);
    const sync = () => has.set(!!get());
    subs.push(
      code.onDidChangeModel(sync),
      code.onDidFocusEditorText(sync),
      code.addAction({ id: "gitviber.addReviewNote", label: "Add Review Note…", contextMenuGroupId: "0_review", precondition: "gitviberNotes", run: () => begin(side) }),
    );
    sync();
  }
  const unsubscribe = subscribeNotes(update);

  return {
    update,
    /** The Add Review Note key: on the side with focus. */
    add: () => begin(isDiff(e) && e.getOriginalEditor().hasWidgetFocus() ? "original" : "modified"),
    dispose() {
      clear();
      dropDraftZone();
      unsubscribe();
      subs.forEach((d) => d.dispose());
    },
  };
}

/** A note as it shows under its lines; labeled with `title`, as a zone's own React root has no tooltips. */
export function NoteCard({ note }: { note: ReviewNote }) {
  return (
    <div className="mx-3 my-1.5 overflow-hidden rounded-md border border-border-strong bg-panel font-sans text-[12px] select-text">
      <div className="flex items-center gap-1.5 pt-1 pr-1 pl-3 text-[11.5px]">
        <MessageSquareText className="size-3.5 shrink-0 text-primary" />
        <span className="font-medium text-foreground">Review note</span>
        <span className="text-subtle">
          on {noteLines(note)}
          {note.old && " (old)"}
        </span>
        <div className="ml-auto flex">
          <CardButton label="Resolve note" onClick={() => updateNote(note.id, { resolved: true })}>
            <Check />
          </CardButton>
          <CardButton label="Delete note" onClick={() => removeNote(note.id)}>
            <Trash2 />
          </CardButton>
        </div>
      </div>
      <div className="px-3 pb-2 whitespace-pre-wrap text-foreground">{note.body}</div>
    </div>
  );
}

function CardButton({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      aria-label={label}
      title={label}
      onClick={onClick}
      className="flex size-6 items-center justify-center rounded-sm text-subtle outline-none hover:bg-hover hover:text-foreground focus-visible:bg-hover focus-visible:text-foreground focus-visible:ring-1 focus-visible:ring-ring [&_svg]:size-3.5"
    >
      {children}
    </button>
  );
}
