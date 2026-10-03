import { Bot, Check, Eye, GitMerge } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { api, errorMessage, type FileChange, type Operation } from "@/lib/api";
import { showLanguage } from "@/lib/editor/shownLanguage";
import { copyNarrowed, indentUnit } from "@/lib/editor/indent";
import { languageFor } from "@/lib/editor/language";
import { basesFor, type Block, endsWithNewline, mergingWhat, oursText, parseConflicts, resolvePrompt, type Segment } from "@/lib/git/conflicts";
import { forTerminal } from "@/lib/review/notes";
import { pasteToAgent } from "@/lib/terminal/terminals";
import { copyText } from "@/lib/app/clipboard";
import { failed, toast } from "@/lib/app/toast";
import { mergeInTool, toolCanOpen, toolName, useExternalTools, useMergingInTool } from "@/lib/git/externalTools";
import { cn } from "@/lib/utils";
import { FileIcon } from "@/components/FileIcon";
import { PathLabel } from "@/components/StatusBadge";
import { type Choice, CodeLines, ConflictCard, IndentUnit, TextRun } from "./ConflictCard";
import { useAsyncValue } from "@/hooks/useAsyncValue";

interface Props {
  file: FileChange;
  /** The worktree's root. */
  root: string;
  branch: string | null;
  /** Every conflicted file's path, for the agent's prompt. */
  conflicted: string[];
  operation: Operation | null;
  revision: number;
}

export function ConflictView({ file, root, branch, conflicted, operation, revision }: Props) {
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
      .catch(failed("Could not read file"));
  }, [file.path, revision, choices.size]);

  // Non-UTF-8 text can't be edited safely here (it was decoded lossily): whole-file only.
  const parsed = useMemo(() => (text == null || lossy ? null : parseConflicts(text)), [text, lossy]);
  const unterminated = text != null && !lossy && parsed === null;
  // Detected once from the whole file (the fragments are too short to sniff), minus the markers,
  // which would make JSON look like YAML. Without a parse only the name decides.
  const lang = useMemo(() => languageFor(file.path, parsed ? oursText(parsed.segments) : undefined), [file.path, parsed]);
  useEffect(() => {
    showLanguage(lang);
    return () => showLanguage(null);
  }, [lang]);
  // CRLF files: the textarea normalises to \n, so custom edits get \r added back on save.
  const crlf = useMemo(() => !!text && text.split("\n").filter((l) => l.endsWith("\r")).length * 2 > text.split("\n").length, [text]);
  // From the whole file: the fragments shown are too short to tell.
  const unit = useMemo(() => indentUnit(text), [text]);
  const blocks = parsed?.segments.filter((s): s is Extract<Segment, { t: "conflict" }> => s.t === "conflict") ?? [];
  const resolved = blocks.filter((b) => choices.has(b.id)).length;
  // Each conflict's base, from the index stages unless the file's diff3 style wrote it: asked for
  // the text read, and kept with it, so another file's never shows.
  // Also how each side ends, for a conflict at the end of the file (endsWithNewline).
  const rebuilt = useAsyncValue(
    text != null && blocks.length
      ? () =>
          api.conflictBase(file.path).then((out) => ({
            text,
            sides: out,
            blocks: (out?.merged && parseConflicts(out.merged)?.segments.filter((s) => s.t === "conflict")) || [],
          }))
      : null,
    [file.path, text, blocks.length > 0],
    null as { text: string; sides: Awaited<ReturnType<typeof api.conflictBase>>; blocks: Block[] } | null,
  );
  const current = rebuilt?.text === text ? rebuilt : null;
  const bases = useMemo(() => basesFor(blocks, current?.blocks ?? []), [parsed, current]);
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

  // The file as the choices make it; a block not chosen yet keeps its markers (the preview shows them).
  const result = () =>
    parsed!.segments.flatMap((s) => {
      if (s.t === "text") return s.lines;
      const c = choices.get(s.id);
      if (!c) return ["<<<<<<< current", ...s.ours, "=======", ...s.theirs, ">>>>>>> incoming"];
      return c.kind === "custom" && crlf ? c.lines.map((l) => (l.endsWith("\r") ? l : `${l}\r`)) : c.lines;
    });
  const [preview, setPreview] = useState(false);

  const save = async () => {
    if (!parsed) return;
    const out = result();
    setBusy(true);
    try {
      // Something else (an agent, an editor) may have changed the file since we parsed it.
      const now = await api.readFile(file.path);
      if (now.text !== text) {
        toast("error", "File changed on disk", "Your choices were reset so nothing is overwritten. Resolve again.");
        setChoices(new Map());
        return;
      }
      const last = parsed.segments.at(-1);
      const eol = endsWithNewline(parsed, last?.t === "conflict" ? choices.get(last.id)?.kind : undefined, current?.sides ?? null);
      await api.writeFile(file.path, out.join("\n") + (eol ? "\n" : ""));
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

  // mergetool merges text both sides changed; for a deleted side it asks on the terminal.
  const tools = useExternalTools(root);
  const mergeTool = (code === "UU" || code === "AA") && toolCanOpen(file.path) ? tools.merge : null;
  const inTool = useMergingInTool(file.path);

  // Into the terminal an agent runs in, here; Enter is left to you. Never a plain shell, where
  // Enter would run the file names' backticks.
  const askAgent = () => {
    const files = conflicted.length ? conflicted : [file.path];
    const prompt = resolvePrompt(files, mergingWhat(operation?.kind, operation?.subject ?? null, branch, blocks[0]?.theirsLabel || null));
    if (!pasteToAgent(root, forTerminal(prompt)))
      toast("info", "No agent running in this worktree", "Start one in a terminal here, or copy the prompt for it.", { label: "Copy prompt", run: () => void copyText(prompt, "Prompt copied") });
  };

  const oursName = rebase ? "Current (base)" : "Current";
  const theirsName = rebase ? "Incoming (your commit)" : "Incoming";

  return (
    <>
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border pr-2 pl-3">
        <FileIcon path={file.path} />
        <PathLabel path={file.path} className="min-w-0 text-[12px]" />
        <span className="rounded-sm bg-conflict-fill px-1.5 py-px text-[10.5px] font-semibold text-on-status">Conflict</span>
        {blocks.length > 0 && (
          <span className="text-[11.5px] text-muted-foreground">
            <span className={cn("font-semibold", resolved === blocks.length ? "text-added" : "text-foreground")}>{resolved}</span>/{blocks.length} resolved
          </span>
        )}
        <div className="ml-auto flex shrink-0 items-center gap-1">
          <Tip label="Pastes a prompt into the terminal an agent runs in, here">
            <Button variant="secondary" size="sm" onClick={askAgent}>
              <Bot /> Ask agent to resolve
            </Button>
          </Tip>
          {mergeTool && (
            <Button variant="secondary" size="sm" disabled={busy || inTool} onClick={() => void mergeInTool(mergeTool, file.path)}>
              <GitMerge /> {inTool ? `Waiting for ${toolName(mergeTool)}…` : `Open in ${toolName(mergeTool)}`}
            </Button>
          )}
          {blocks.length > 0 && (
            <>
              <Button variant={preview ? "default" : "secondary"} size="sm" aria-pressed={preview} onClick={() => setPreview((p) => !p)}>
                <Eye /> Preview result
              </Button>
              <Button variant="secondary" size="sm" onClick={() => chooseAll("ours")}>
                All current
              </Button>
              <Button variant="secondary" size="sm" onClick={() => chooseAll("theirs")}>
                All incoming
              </Button>
              <Button size="sm" disabled={busy || resolved < blocks.length} onClick={save}>
                <Check /> Mark resolved
              </Button>
            </>
          )}
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-auto" onCopy={copyNarrowed(unit)}>
        <IndentUnit.Provider value={unit}>
          {text == null ? null : blocks.length === 0 ? (
            <WholeFile code={code} busy={busy} onTake={takeSide} onAsIs={markAsIs} rebase={rebase} lossy={lossy} unterminated={unterminated} />
          ) : preview ? (
            <div className="py-2">
              <CodeLines lines={result().map((l) => l.replace(/\r$/, ""))} lang={lang} />
            </div>
          ) : (
            <div className="py-2">
              {parsed!.segments.map((seg, i) =>
                seg.t === "text" ? (
                  <TextRun key={`t${i}`} lines={seg.lines} lang={lang} />
                ) : (
                  <ConflictCard
                    key={`c${seg.id}`}
                    block={seg}
                    index={seg.id + 1}
                    total={blocks.length}
                    choice={choices.get(seg.id)}
                    base={bases[seg.id] ?? null}
                    lang={lang}
                    oursName={oursName}
                    theirsName={theirsName}
                    onChoose={(k, lines) => choose(seg, k, lines)}
                    onUndo={() => undo(seg)}
                  />
                ),
              )}
            </div>
          )}
        </IndentUnit.Provider>
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
