import { Check, ChevronsUpDown, GitMerge, Pencil, Undo2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { api, errorMessage, type FileChange, type Operation } from "@/lib/api";
import { languageFor, type TokenLine, tokenLookup, useHighlight } from "@/lib/highlight";
import { CODE_FONTS, useSettings } from "@/lib/settings";
import { type Block, parseConflicts, type Segment } from "@/lib/conflicts";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { FileIcon } from "./FileIcon";
import { PathLabel } from "./StatusBadge";

type Choice = { kind: "ours" | "theirs" | "both" | "custom"; lines: string[] };

interface Props {
  file: FileChange;
  operation: Operation | null;
  revision: number;
}

export function ConflictView({ file, operation, revision }: Props) {
  const [text, setText] = useState<string | null>(null);
  const [lossy, setLossy] = useState(false);
  const [choices, setChoices] = useState<Map<number, Choice>>(() => new Map());
  const [busy, setBusy] = useState(false);
  const code = file.conflict ?? "UU";

  // Only reload from disk before the user starts resolving, so edits in progress survive refreshes.
  useEffect(() => {
    if (choices.size) return;
    api
      .readFile(file.path)
      .then((f) => {
        setText(f.exists && !f.binary && !f.tooLarge ? f.text : "");
        setLossy(f.lossy);
      })
      .catch((e) => toast("error", "Could not read file", errorMessage(e)));
  }, [file.path, revision, choices.size]);

  // Non-UTF-8 text can't be edited safely here (it was decoded lossily): whole-file only.
  const parsed = useMemo(() => (text == null || lossy ? null : parseConflicts(text)), [text, lossy]);
  const unterminated = text != null && !lossy && parsed === null;
  // CRLF files: the textarea normalises to \n, so custom edits get \r added back on save.
  const crlf = useMemo(() => !!text && text.split("\n").filter((l) => l.endsWith("\r")).length * 2 > text.split("\n").length, [text]);
  const blocks = parsed?.segments.filter((s): s is Extract<Segment, { t: "conflict" }> => s.t === "conflict") ?? [];
  const resolved = blocks.filter((b) => choices.has(b.id)).length;
  // In a rebase HEAD is the branch you're rebasing onto; "incoming" is your own commit being replayed.
  const rebase = operation?.kind === "rebase";

  const choose = (b: Block, kind: Choice["kind"], lines?: string[]) =>
    setChoices((m) => new Map(m).set(b.id, { kind, lines: lines ?? (kind === "ours" ? b.ours : kind === "theirs" ? b.theirs : [...b.ours, ...b.theirs]) }));
  const undo = (b: Block) =>
    setChoices((m) => {
      const next = new Map(m);
      next.delete(b.id);
      return next;
    });
  const chooseAll = (kind: "ours" | "theirs") => blocks.forEach((b) => choose(b, kind));

  const save = async () => {
    if (!parsed) return;
    const out = parsed.segments.flatMap((s) => {
      if (s.t === "text") return s.lines;
      const c = choices.get(s.id)!;
      return c.kind === "custom" && crlf ? c.lines.map((l) => (l.endsWith("\r") ? l : `${l}\r`)) : c.lines;
    });
    setBusy(true);
    try {
      // Something else (an agent, an editor) may have changed the file since we parsed it.
      const now = await api.readFile(file.path);
      if (now.text !== text) {
        toast("error", "File changed on disk", "Your choices were reset so nothing is overwritten. Resolve again.");
        setChoices(new Map());
        return;
      }
      await api.writeFile(file.path, out.join("\n") + (parsed.trailingNewline ? "\n" : ""));
      await api.stage([file.path]);
      toast("success", "Conflict resolved", file.path);
    } catch (e) {
      toast("error", "Could not save resolution", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const markAsIs = async () => {
    setBusy(true);
    try {
      await api.stage([file.path]);
      toast("success", "Marked resolved", file.path);
    } catch (e) {
      toast("error", "Could not mark resolved", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const takeSide = async (side: "ours" | "theirs") => {
    setBusy(true);
    try {
      await api.resolveSide(file.path, side);
      toast("success", "Conflict resolved", file.path);
    } catch (e) {
      toast("error", "Could not resolve", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const oursName = rebase ? "Current (base)" : "Current";
  const theirsName = rebase ? "Incoming (your commit)" : "Incoming";

  return (
    <>
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border pr-2 pl-3">
        <FileIcon path={file.path} />
        <PathLabel path={file.path} className="min-w-0 text-[12px]" />
        <span className="rounded-sm bg-conflict px-1.5 py-px text-[10.5px] font-semibold text-black/85">Conflict</span>
        {blocks.length > 0 && (
          <span className="text-[11.5px] text-muted-foreground">
            <span className={cn("font-semibold", resolved === blocks.length ? "text-added" : "text-foreground")}>{resolved}</span>/{blocks.length} resolved
          </span>
        )}
        {blocks.length > 0 && (
          <div className="ml-auto flex shrink-0 items-center gap-1">
            <Button variant="secondary" size="sm" onClick={() => chooseAll("ours")}>
              All current
            </Button>
            <Button variant="secondary" size="sm" onClick={() => chooseAll("theirs")}>
              All incoming
            </Button>
            <Button size="sm" disabled={busy || resolved < blocks.length} onClick={save}>
              <Check /> Mark resolved
            </Button>
          </div>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {text == null ? null : blocks.length === 0 ? (
          <WholeFile code={code} busy={busy} onTake={takeSide} onAsIs={markAsIs} rebase={rebase} lossy={lossy} unterminated={unterminated} />
        ) : (
          <div className="py-2">
            {parsed!.segments.map((seg, i) =>
              seg.t === "text" ? (
                <TextRun key={`t${i}`} lines={seg.lines} path={file.path} />
              ) : (
                <ConflictCard
                  key={`c${seg.id}`}
                  block={seg}
                  index={seg.id + 1}
                  total={blocks.length}
                  choice={choices.get(seg.id)}
                  path={file.path}
                  oursName={oursName}
                  theirsName={theirsName}
                  onChoose={(k, lines) => choose(seg, k, lines)}
                  onUndo={() => undo(seg)}
                />
              ),
            )}
          </div>
        )}
      </div>
    </>
  );
}

/** Conflicts without text markers (deletions, binaries): pick a side for the whole file. */
function WholeFile({
  code,
  busy,
  onTake,
  onAsIs,
  rebase,
  lossy,
  unterminated,
}: {
  code: string;
  busy: boolean;
  onTake: (s: "ours" | "theirs") => void;
  onAsIs: () => void;
  rebase: boolean;
  lossy: boolean;
  unterminated: boolean;
}) {
  const note: Record<string, [string, string]> = {
    UD: ["Modified on the current side", "Deleted on the incoming side"],
    DU: ["Deleted on the current side", "Modified on the incoming side"],
    AU: ["Added on the current side", "Not on the incoming side"],
    UA: ["Not on the current side", "Added on the incoming side"],
    DD: ["Deleted on both sides", "Deleted on both sides"],
  };
  const [ours, theirs] = note[code] ?? ["Current version", "Incoming version"];
  // A side "deletes" the file if it removed it (D) or never had it (U next to the other's A).
  const deletes = (side: "ours" | "theirs") => {
    const [mine, other] = side === "ours" ? [code[0], code[1]] : [code[1], code[0]];
    return mine === "D" || (mine === "U" && other === "A");
  };
  // Both sides have text but no markers are left: someone already merged it by hand.
  const handMerged = (code === "UU" || code === "AA") && !lossy && !unterminated;
  return (
    <div className="mx-auto mt-16 max-w-lg px-6 text-center">
      <GitMerge className="mx-auto size-7 text-conflict" />
      <div className="mt-3 text-[13px] font-medium">
        {handMerged ? "No conflict markers left" : unterminated ? "The conflict markers are incomplete" : "This conflict can't be resolved line by line"}
      </div>
      <div className="mt-1 text-[12px] text-muted-foreground">
        {handMerged
          ? "The file was already merged by hand. Mark it resolved as it is, or replace it with one side."
          : lossy
            ? "The file isn't UTF-8, so it can't be edited here safely. Keep one side, or fix it in your editor and mark it resolved."
            : unterminated
              ? "Fix the markers in your editor, or keep one side."
              : `Choose which side of the file to keep${rebase ? " (in a rebase, “incoming” is your own commit)" : ""}.`}
      </div>
      {(handMerged || lossy || unterminated) && (
        <Button className="mt-4" disabled={busy} onClick={onAsIs}>
          <Check /> Mark resolved as it is
        </Button>
      )}
      <div className="mt-5 grid grid-cols-2 gap-2">
        {(["ours", "theirs"] as const).map((side) => (
          <button
            key={side}
            disabled={busy}
            onClick={() => onTake(side)}
            className="rounded-md border border-border-strong bg-panel p-3 text-left hover:border-primary disabled:opacity-50"
          >
            <div className="text-[12.5px] font-semibold">{deletes(side) ? "Delete the file" : side === "ours" ? "Keep current" : "Keep incoming"}</div>
            <div className="mt-0.5 text-[11.5px] text-muted-foreground">{side === "ours" ? ours : theirs}</div>
          </button>
        ))}
      </div>
    </div>
  );
}

function useCodeStyle() {
  const s = useSettings();
  return { fontFamily: CODE_FONTS[s.codeFont], fontSize: s.codeFontSize, lineHeight: `${Math.round(s.codeFontSize * s.lineHeight)}px`, tabSize: 4 } as const;
}

function CodeLines({ lines, path, className }: { lines: string[]; path: string; className?: string }) {
  const s = useSettings();
  const style = useCodeStyle();
  const hl = useHighlight(lines.join("\n"), languageFor(path), s.syntaxTheme);
  const tok = useMemo(() => tokenLookup(hl), [hl]);
  return (
    <div className={cn("overflow-x-auto px-4 whitespace-pre select-text", className)} style={{ ...style, color: hl?.data.fg }}>
      {lines.map((l, i) => (
        <div key={i} className="min-h-[1lh]">
          <Tokens tokens={tok(i, l)} text={l} />
        </div>
      ))}
    </div>
  );
}

function Tokens({ tokens, text }: { tokens?: TokenLine; text: string }) {
  if (!tokens) return <>{text}</>;
  return (
    <>
      {tokens.map(([t, color, fs], i) => (
        <span key={i} style={{ color: color || undefined, fontStyle: fs & 1 ? "italic" : undefined, fontWeight: fs & 2 ? 600 : undefined }}>
          {t}
        </span>
      ))}
    </>
  );
}

const CONTEXT = 3;

/** Unchanged text between conflicts, folded to a few lines of context. */
function TextRun({ lines, path }: { lines: string[]; path: string }) {
  const [open, setOpen] = useState(false);
  if (open || lines.length <= CONTEXT * 2 + 1) return <CodeLines lines={lines} path={path} className="text-foreground/70" />;
  return (
    <>
      <CodeLines lines={lines.slice(0, CONTEXT)} path={path} className="text-foreground/70" />
      <button onClick={() => setOpen(true)} className="flex w-full items-center gap-2 border-y border-border bg-panel px-4 py-1 text-[11.5px] text-subtle hover:bg-elevated hover:text-foreground">
        <ChevronsUpDown className="size-3.5" /> {lines.length - CONTEXT * 2} unchanged lines
      </button>
      <CodeLines lines={lines.slice(-CONTEXT)} path={path} className="text-foreground/70" />
    </>
  );
}

function ConflictCard({
  block,
  index,
  total,
  choice,
  path,
  oursName,
  theirsName,
  onChoose,
  onUndo,
}: {
  block: Block;
  index: number;
  total: number;
  choice?: Choice;
  path: string;
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
        <CodeLines lines={choice.lines} path={path} className="bg-add-bg py-1" />
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
          <Side title={oursName} ref_={block.oursLabel} tone="bg-primary/10 border-primary" lines={block.ours} path={path} />
          {block.base && <Side title="Common ancestor" ref_="" tone="bg-hover border-subtle" lines={block.base} path={path} />}
          <Side title={theirsName} ref_={block.theirsLabel} tone="bg-renamed/10 border-renamed" lines={block.theirs} path={path} />
        </>
      )}
    </div>
  );
}

function Side({ title, ref_, tone, lines, path }: { title: string; ref_: string; tone: string; lines: string[]; path: string }) {
  return (
    <div className={cn("border-l-2", tone)}>
      <div className="px-4 pt-1.5 text-[10.5px] font-semibold tracking-[0.06em] text-muted-foreground uppercase">
        {title}
        {ref_ && <span className="ml-2 font-mono tracking-normal normal-case text-subtle">{ref_}</span>}
      </div>
      {lines.length ? <CodeLines lines={lines} path={path} className="pb-1.5" /> : <div className="px-4 pb-1.5 text-[11.5px] text-subtle italic">(empty)</div>}
    </div>
  );
}
