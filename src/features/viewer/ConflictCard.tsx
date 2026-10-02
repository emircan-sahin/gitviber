import { ChevronRight, ChevronsUpDown, Pencil, Undo2 } from "lucide-react";
import { createContext, useContext, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { tokenLookup, useHighlight } from "@/lib/editor/highlight";
import { widenLine } from "@/lib/editor/indent";
import { useSettings } from "@/lib/settings";
import type { Block } from "@/lib/git/conflicts";
import { cn } from "@/lib/utils";
import { Tokens, useCodeStyle } from "./codeLines";

// One conflict block of ConflictView, and the code it shows between them.

export type Choice = { kind: "ours" | "theirs" | "both" | "custom"; lines: string[] };

/** Spaces per indentation level the file's code is shown widened from (see lib/editor/indent). */
export const IndentUnit = createContext(0);

export function CodeLines({ lines: raw, lang, className }: { lines: string[]; lang: string; className?: string }) {
  const s = useSettings();
  const style = useCodeStyle();
  const unit = useContext(IndentUnit);
  const lines = useMemo(() => (unit ? raw.map((l) => widenLine(l, unit)) : raw), [raw, unit]);
  const hl = useHighlight(lines.join("\n"), lang, s.codeTheme);
  const tok = useMemo(() => tokenLookup(hl), [hl]);
  return (
    <div
      // The I-beam over the whole block, as an editor has it, not only over the letters.
      className={cn("cursor-text px-4 select-text", s.wordWrap ? "whitespace-pre-wrap [overflow-wrap:anywhere]" : "overflow-x-auto whitespace-pre", className)}
      style={{ ...style, color: hl?.data.fg }}
    >
      {lines.map((l, i) => (
        <div key={i} className="min-h-[1lh]">
          <Tokens tokens={tok(i, l)} text={l} />
        </div>
      ))}
    </div>
  );
}

const CONTEXT = 3;

/** Unchanged text between conflicts, folded to a few lines of context. */
export function TextRun({ lines, lang }: { lines: string[]; lang: string }) {
  const [open, setOpen] = useState(false);
  if (open || lines.length <= CONTEXT * 2 + 1) return <CodeLines lines={lines} lang={lang} className="text-foreground/70" />;
  return (
    <>
      <CodeLines lines={lines.slice(0, CONTEXT)} lang={lang} className="text-foreground/70" />
      <button onClick={() => setOpen(true)} className="flex w-full items-center gap-2 border-y border-border bg-panel px-4 py-1 text-[11.5px] text-subtle hover:bg-elevated focus-visible:bg-elevated hover:text-foreground focus-visible:text-foreground">
        <ChevronsUpDown className="size-3.5" /> {lines.length - CONTEXT * 2} unchanged lines
      </button>
      <CodeLines lines={lines.slice(-CONTEXT)} lang={lang} className="text-foreground/70" />
    </>
  );
}

export function ConflictCard({
  block,
  index,
  total,
  choice,
  base,
  lang,
  oursName,
  theirsName,
  onChoose,
  onUndo,
}: {
  block: Block;
  index: number;
  total: number;
  choice?: Choice;
  /** What both sides changed: the merge base's lines here, null when unknown. */
  base: string[] | null;
  lang: string;
  oursName: string;
  theirsName: string;
  onChoose: (kind: Choice["kind"], lines?: string[]) => void;
  onUndo: () => void;
}) {
  const [editing, setEditing] = useState<string | null>(null);
  const style = useCodeStyle();
  const label = { ours: `Accepted ${oursName.toLowerCase()}`, theirs: `Accepted ${theirsName.toLowerCase()}`, both: "Accepted both", custom: "Edited" };

  return (
    <div className={cn("mx-3 my-2 overflow-hidden rounded-md border", choice ? "border-added/50" : "border-conflict/60")}>
      <div className="flex h-8 items-center gap-2 border-b border-border bg-panel px-3 text-[11.5px]">
        <span className={cn("font-semibold", choice ? "text-added" : "text-conflict")}>
          Conflict {index} of {total}
        </span>
        {choice && <span className="text-muted-foreground">· {label[choice.kind]}</span>}
        <div className="ml-auto flex items-center gap-1">
          {choice ? (
            <Button variant="ghost" size="sm" onClick={onUndo}>
              <Undo2 /> Undo
            </Button>
          ) : editing == null ? (
            <>
              <Button variant="secondary" size="sm" onClick={() => onChoose("ours")}>
                Accept current
              </Button>
              <Button variant="secondary" size="sm" onClick={() => onChoose("theirs")}>
                Accept incoming
              </Button>
              <Button variant="secondary" size="sm" onClick={() => onChoose("both")}>
                Accept both
              </Button>
              <Button variant="ghost" size="sm" onClick={() => setEditing([...block.ours, ...block.theirs].join("\n"))}>
                <Pencil /> Edit
              </Button>
            </>
          ) : (
            <>
              <Button variant="ghost" size="sm" onClick={() => setEditing(null)}>
                Cancel
              </Button>
              <Button
                size="sm"
                onClick={() => {
                  onChoose("custom", editing.split("\n"));
                  setEditing(null);
                }}
              >
                Use this
              </Button>
            </>
          )}
        </div>
      </div>
      {choice ? (
        <CodeLines lines={choice.lines} lang={lang} className="bg-add-bg py-1" />
      ) : editing != null ? (
        <textarea
          autoFocus
          value={editing}
          onChange={(e) => setEditing(e.target.value)}
          spellCheck={false}
          rows={Math.min(24, editing.split("\n").length + 1)}
          className="block w-full resize-y bg-background px-4 py-1 text-foreground outline-none select-text"
          style={style}
        />
      ) : (
        <>
          <Side title={oursName} ref_={block.oursLabel} tone="bg-primary/10 border-primary" lines={block.ours} lang={lang} />
          {base && <BaseStrip lines={base} lang={lang} open={!!block.base} />}
          <Side title={theirsName} ref_={block.theirsLabel} tone="bg-renamed/10 border-renamed" lines={block.theirs} lang={lang} />
        </>
      )}
    </div>
  );
}

function Side({ title, ref_, tone, lines, lang }: { title: string; ref_: string; tone: string; lines: string[]; lang: string }) {
  return (
    <div className={cn("border-l-2", tone)}>
      <div className="px-4 pt-1.5 text-[10.5px] font-semibold tracking-[0.06em] text-muted-foreground uppercase">
        {title}
        {ref_ && <span className="ml-2 font-mono tracking-normal normal-case text-subtle">{ref_}</span>}
      </div>
      {lines.length ? <CodeLines lines={lines} lang={lang} className="pb-1.5" /> : <div className="px-4 pb-1.5 text-[11.5px] text-subtle italic">(empty)</div>}
    </div>
  );
}

/**
 * The merge base between the two sides, folded to a strip: open to see what each side changed.
 * Open from the start where the user's conflictstyle (diff3, zdiff3) asked for it in the file.
 */
function BaseStrip({ lines, lang, open: initial }: { lines: string[]; lang: string; open: boolean }) {
  const [open, setOpen] = useState(initial);
  return (
    <div className="border-l-2 border-subtle bg-hover">
      <button
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="flex w-full items-center gap-1 px-4 py-1 text-left text-[10.5px] font-semibold tracking-[0.06em] text-muted-foreground uppercase hover:text-foreground focus-visible:text-foreground"
      >
        <ChevronRight className={cn("size-3 shrink-0 transition-transform", open && "rotate-90")} />
        Base
        <span className="ml-1 font-normal tracking-normal normal-case text-subtle">{lines.length ? `${lines.length} ${lines.length === 1 ? "line" : "lines"} before either side changed them` : "empty: both sides added these lines"}</span>
      </button>
      {open && lines.length > 0 && <CodeLines lines={lines} lang={lang} className="pb-1.5" />}
    </div>
  );
}
