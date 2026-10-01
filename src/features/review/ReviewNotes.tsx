import { Check, Copy, MessageSquareText, Trash2, Undo2 } from "lucide-react";
import { RowAction } from "@/components/RowAction";
import { copyText } from "@/lib/app/clipboard";
import { toast } from "@/lib/app/toast";
import { openTarget } from "@/lib/links/linkHost";
import type { Selection } from "@/lib/repo/selection";
import { forTerminal, isLive, noteLocation, notesPrompt, type ReviewNote } from "@/lib/review/notes";
import { clearResolved, getNotes, notesRoot, removeNote, updateNote, useNotes } from "@/lib/review/noteStore";
import { pasteToWorktree } from "@/lib/terminal/terminals";
import { cn } from "@/lib/utils";
import { Section, SectionBtn } from "@/features/changes/ChangeRows";

/** Open notes first, each file's in line order. */
const ordered = (notes: ReviewNote[]) =>
  [...notes].sort((a, b) => Number(!!a.resolved) - Number(!!b.resolved) || a.path.localeCompare(b.path) || a.start - b.start);

/** The notes not resolved yet, in the list's order: what goes to the agent. */
export const pendingNotes = () => ordered(getNotes()).filter((n) => !n.resolved);

const noNotes = () => toast("info", "No open review notes", "Add one from a diff's right-click menu or with its key.");

export function copyNotes(notes: ReviewNote[]) {
  if (!notes.length) return noNotes();
  void copyText(notesPrompt(notes), notes.length === 1 ? "Note copied for the agent" : `${notes.length} notes copied for the agent`, "Paste it into the agent's terminal.");
}

/** Into this worktree's terminal, where the agent runs; Enter is left to you. */
export function sendNotes(notes: ReviewNote[]) {
  const root = notesRoot();
  if (!notes.length) return noNotes();
  if (root && !pasteToWorktree(root, forTerminal(notesPrompt(notes)))) toast("info", "No terminal in this worktree", "Open one and start the agent in it, or copy the notes instead.");
}

/**
 * The worktree's review notes, a section of Changes. `changes`: the list it's in, where a note's
 * file is looked up when its lines aren't on disk to go to.
 */
export function ReviewNotes({ changes, onOpen }: { changes: Selection[]; onOpen: (s: Selection) => void }) {
  const notes = ordered(useNotes());
  const pending = notes.filter((n) => !n.resolved);
  const resolved = notes.length - pending.length;
  // Its lines in the file, where it shows under them; else the diff that has the file.
  const goTo = (n: ReviewNote) => {
    if (isLive(n) && !n.outdated) return openTarget({ path: n.path, line: n.start, column: 1 }, true);
    const change = changes.find((c) => "file" in c && c.file.path === n.path);
    if (change) onOpen(change);
    else toast("info", "Not among the changes", `${n.path} has no changes${n.at ? ` here; the note is on ${n.at}` : ""}.`);
  };
  return (
    <Section
      title="Review Notes"
      count={pending.length}
      action={
        <>
          {pending.length > 0 && <SectionBtn onClick={() => copyNotes(pending)}>Copy as Prompt</SectionBtn>}
          {pending.length > 0 && <SectionBtn onClick={() => sendNotes(pending)}>Send to Terminal</SectionBtn>}
          {resolved > 0 && <SectionBtn onClick={clearResolved}>Clear Resolved</SectionBtn>}
        </>
      }
    >
      {notes.map((n) => (
        <div key={n.id} className="group/row flex items-start gap-2 py-1 pr-2 pl-2 text-[12px] focus-within:bg-hover hover:bg-hover">
          <MessageSquareText className={cn("mt-0.5 size-3.5 shrink-0", n.resolved ? "text-subtle" : "text-primary")} />
          <button onClick={() => goTo(n)} aria-label={`Go to the note on ${noteLocation(n)}: ${n.body}`} className="min-w-0 flex-1 rounded-sm text-left outline-none focus-visible:ring-1 focus-visible:ring-ring">
            <span className="flex items-baseline gap-1.5">
              <span className="truncate font-mono text-[11px] text-muted-foreground">{noteLocation(n)}</span>
              {n.old && <Badge>old</Badge>}
              {n.at && <Badge>{n.at}</Badge>}
              {n.outdated && !n.resolved && <Badge tone="text-modified">outdated</Badge>}
            </span>
            <span className={cn("block truncate", n.resolved ? "text-subtle line-through" : "text-foreground")}>{n.body}</span>
          </button>
          <div className="hidden shrink-0 items-center group-focus-within/row:flex group-hover/row:flex">
            {!n.resolved && (
              <RowAction label="Copy as prompt" onClick={() => copyNotes([n])}>
                <Copy />
              </RowAction>
            )}
            <RowAction label={n.resolved ? "Reopen note" : "Resolve note"} onClick={() => updateNote(n.id, { resolved: !n.resolved })}>
              {n.resolved ? <Undo2 /> : <Check />}
            </RowAction>
            <RowAction label="Delete note" onClick={() => removeNote(n.id)}>
              <Trash2 />
            </RowAction>
          </div>
        </div>
      ))}
    </Section>
  );
}

function Badge({ tone = "text-subtle", children }: { tone?: string; children: React.ReactNode }) {
  return <span className={cn("shrink-0 rounded-sm bg-elevated px-1 font-mono text-[10px] leading-4", tone)}>{children}</span>;
}
