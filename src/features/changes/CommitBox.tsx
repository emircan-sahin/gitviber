import { ArrowUpFromLine, Ellipsis, LoaderCircle, RefreshCw, ShieldOff, Signature, Sparkles, TriangleAlert, UserPlus } from "lucide-react";
import { useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Tip } from "@/components/ui/tooltip";
import { api, type Commit, type RepoStatus } from "@/lib/api";
import { matchesCommand, runCommand, useCommands, useShortcut } from "@/lib/commands/keybindings";
import { updateSettings, useSettings } from "@/lib/settings";
import { toast } from "@/lib/app/toast";
import { tracked, undoAction } from "@/lib/repo/undo";
import { cn } from "@/lib/utils";
import { NESTED_EXPLAINED, stageable } from "@/lib/git/worktrees";
import { attempt, files, leftOut } from "./changeList";
import { SectionBtn } from "./ChangeRows";
import { CoAuthorChip, CoAuthorPicker, OptionChip } from "./CoAuthorPicker";
import { useCommitDraft, useSuggestMessage } from "./useCommitBox";

/** Past this, `git log --oneline` and GitHub cut the summary off. */
const SUMMARY_LIMIT = 72;

/** `shown`: what the list's filter leaves, while it has text; the button says how many files it takes that the list hides. */
export function CommitBox({ status, shown, head, main, refresh }: { status: RepoStatus; shown: RepoStatus | null; head: Commit | null; main: string; refresh: () => Promise<void> }) {
  const { draft, setDraft, amend, edited, template, toggleAmend, clear } = useCommitDraft(status.root, head);
  const [busy, setBusy] = useState(false);
  const { signOffRepos, suggestEnabled } = useSettings();
  const signOff = signOffRepos.includes(main);
  const setSignOff = (on: boolean) => updateSettings({ signOffRepos: on ? [...signOffRepos, main] : signOffRepos.filter((r) => r !== main) });
  // One commit only: a hook that's broken today shouldn't be skipped forever.
  const [noVerify, setNoVerify] = useState(false);
  const [addingCoAuthor, setAddingCoAuthor] = useState(false);
  // Set by Add co-author: focus going back to the options button would steal it from the picker.
  const pickingCoAuthor = useRef(false);

  const onAmend = (on: boolean) => {
    dropSuggestion();
    toggleAmend(on);
  };

  const summary = draft.summary.trim();
  const hasStaged = status.staged.length > 0;
  const all = stageable(status.unstaged);
  const hasAny = hasStaged || all.paths.length > 0;
  const canCommit = !busy && !status.conflicted.length && (amend ? !edited || summary !== "" : summary !== "" && hasAny);
  const label = amend ? "Amend" : hasStaged ? "Commit" : "Commit all";
  // The button stays short; the tooltip still says how much goes in.
  const scope = hasStaged && !amend ? `Commit ${status.staged.length} staged` : label;
  const target = status.branch ? `${scope} to ${status.branch}` : scope;
  const skipped = !hasStaged && !amend && all.skipped > 0;
  const committed = hasStaged || amend ? status.staged.map((f) => f.path) : all.paths;
  const visible = new Set(shown ? [...shown.staged, ...shown.unstaged].map((f) => f.path) : committed);
  const hidden = committed.filter((p) => !visible.has(p)).length;
  // `unpushed` is also false when there's nothing to compare with, so only a pushed branch can tell.
  const pushed = !!amend && !!head && !head.unpushed && !!(status.upstream || status.push?.branch);
  const length = [...draft.summary].length;

  // `then`: push or sync right after, as VS Code's Commit & Push (the top bar's commands do it).
  const commit = async (then?: "git.push" | "git.sync") => {
    if (!canCommit) return;
    dropSuggestion();
    setBusy(true);
    const body = draft.body.trim();
    // An untouched amend message goes as none, so git keeps the original exactly.
    const message = amend && !edited ? "" : body ? `${summary}\n\n${body}` : summary;
    let entry: number | null = null;
    const ok = await attempt("Commit failed", async () => {
      // Nothing staged means "commit everything", the common case after an agent run.
      if (!hasStaged && !amend) await api.stage(all.paths);
      [, entry] = await tracked(() => api.commit(message, { amend: !!amend, signOff, noVerify, coAuthors: draft.coAuthors }));
    });
    setBusy(false);
    if (ok) {
      clear();
      setNoVerify(false);
      toast("success", amend ? "Commit amended" : "Committed", summary, undoAction(entry, refresh));
      // They stay in the list after the commit; say why rather than leave it looking missed.
      if (skipped) toast("info", `${leftOut(all.skipped)} of the commit`, NESTED_EXPLAINED);
    }
    await refresh();
    if (ok && then) runCommand(then);
  };

  const { suggesting, program, canSuggest, cancelSuggest, dropSuggestion, suggest } = useSuggestMessage({ draft, setDraft, template, amend: !!amend, hasStaged, hasAny, busy });

  useCommands({ "git.commit": canCommit ? commit : undefined, "git.suggestMessage": canSuggest ? suggest : undefined });
  const commitKey = useShortcut("git.commit");
  const suggestKey = useShortcut("git.suggestMessage");
  const onKey = (e: React.KeyboardEvent) => {
    if (matchesCommand("git.commit", e.nativeEvent)) {
      e.preventDefault();
      commit();
    }
  };

  return (
    <div className="shrink-0 border-t border-border bg-panel p-2">
      <div className="relative">
        <Input placeholder="Summary" value={draft.summary} onChange={(e) => setDraft({ ...draft, summary: e.target.value })} onKeyDown={onKey} className={cn("font-medium", length > 50 && "pr-8")} />
        {length > 50 && (
          <Tip label={`Summaries over ${SUMMARY_LIMIT} characters get cut off in git log and on GitHub`}>
            <span className={cn("absolute top-1/2 right-2 -translate-y-1/2 font-mono text-[10.5px]", length > SUMMARY_LIMIT ? "text-modified" : "text-subtle")}>{length}</span>
          </Tip>
        )}
      </div>
      <Textarea placeholder="Description" value={draft.body} onChange={(e) => setDraft({ ...draft, body: e.target.value })} onKeyDown={onKey} rows={2} className="mt-1.5 py-1.5 text-[12px]" />
      {(noVerify || signOff || draft.coAuthors.length > 0) && (
        <div className="mt-1.5 flex flex-wrap gap-1">
          {noVerify && (
            <OptionChip warn label="Skip hooks" tip="The pre-commit and commit-msg hooks won't run for this commit" onRemove={() => setNoVerify(false)}>
              <ShieldOff />
            </OptionChip>
          )}
          {signOff && (
            <OptionChip label="Sign off" tip="Adds Signed-off-by with your name, on every commit in this repository" onRemove={() => setSignOff(false)}>
              <Signature />
            </OptionChip>
          )}
          {draft.coAuthors.map((a) => (
            <CoAuthorChip key={a} author={a} onRemove={() => setDraft({ ...draft, coAuthors: draft.coAuthors.filter((x) => x !== a) })} />
          ))}
        </div>
      )}
      <div className="mt-1.5 flex items-center gap-2">
        <label className={cn("flex items-center gap-1.5 text-[11.5px] text-muted-foreground", !head && "opacity-50")}>
          <input type="checkbox" checked={!!amend} disabled={!head} onChange={(e) => onAmend(e.target.checked)} className="accent-primary" />
          Amend
        </label>
        {suggestEnabled && (
          <Tip label={suggesting ? `Stop ${program}` : `Suggest a message with ${program}`} shortcut={suggesting ? undefined : suggestKey}>
            <Button variant="ghost" size="icon" aria-label={suggesting ? "Stop suggesting" : "Suggest a message"} disabled={!suggesting && !canSuggest} onClick={suggesting ? cancelSuggest : suggest}>
              {suggesting ? <LoaderCircle className="animate-spin" /> : <Sparkles />}
            </Button>
          </Tip>
        )}
        <CoAuthorPicker open={addingCoAuthor} onOpenChange={setAddingCoAuthor} taken={draft.coAuthors} onAdd={(a) => setDraft((d) => ({ ...d, coAuthors: [...d.coAuthors, a] }))}>
          <DropdownMenu>
            <Tip label="Commit options">
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="icon" aria-label="Commit options">
                  <Ellipsis />
                </Button>
              </DropdownMenuTrigger>
            </Tip>
            <DropdownMenuContent
              side="top"
              align="end"
              className="w-56"
              onCloseAutoFocus={(e) => {
                if (pickingCoAuthor.current) e.preventDefault();
                pickingCoAuthor.current = false;
              }}
            >
              <DropdownMenuItem
                onSelect={() => {
                  pickingCoAuthor.current = true;
                  setAddingCoAuthor(true);
                }}
              >
                <UserPlus /> Add co-author…
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem disabled={!canCommit} onSelect={() => commit("git.push")}>
                <ArrowUpFromLine /> {label} & Push
              </DropdownMenuItem>
              <DropdownMenuItem disabled={!canCommit} onSelect={() => commit("git.sync")}>
                <RefreshCw /> {label} & Sync
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuCheckboxItem checked={signOff} onCheckedChange={setSignOff}>
                Sign off <span className="ml-auto text-[11px] opacity-60">this repo</span>
              </DropdownMenuCheckboxItem>
              <DropdownMenuCheckboxItem checked={noVerify} onCheckedChange={setNoVerify}>
                Skip hooks <span className="ml-auto text-[11px] opacity-60">this commit</span>
              </DropdownMenuCheckboxItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </CoAuthorPicker>
        <Tip
          label={[target, skipped && leftOut(all.skipped).toLowerCase(), hidden && `including ${files(hidden)} the filter hides`].filter(Boolean).join(", ")}
          shortcut={commitKey}
        >
          <Button className="ml-auto flex-1" disabled={!canCommit} onClick={() => commit()}>
            {busy ? "Committing…" : hidden ? `${label} · ${hidden} hidden` : label}
          </Button>
        </Tip>
      </div>
      {suggesting && (
        <div className="mt-1.5 flex items-center gap-1.5 text-[11px] text-muted-foreground">
          <LoaderCircle className="size-3 shrink-0 animate-spin" />
          <span className="min-w-0 truncate">Asking {program} for a message…</span>
          <span className="ml-auto shrink-0">
            <SectionBtn onClick={cancelSuggest}>Cancel</SectionBtn>
          </span>
        </div>
      )}
      {pushed && (
        <div className="mt-1.5 flex items-center gap-1.5 text-[11px] text-modified">
          <TriangleAlert className="size-3 shrink-0" />
          Already pushed: amending needs a force push
        </div>
      )}
    </div>
  );
}
