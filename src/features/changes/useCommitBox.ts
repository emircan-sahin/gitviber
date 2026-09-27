import { useEffect, useRef, useState } from "react";
import { api, type Commit, errorMessage, SUGGEST_CANCELLED } from "@/lib/api";
import { type CommitDraft, loadDraft, saveDraft } from "@/lib/repo/session";
import { useSettings } from "@/lib/settings";
import { commandLine, parseSuggestion, programOf, SUGGEST_PROMPT } from "@/lib/git/suggest";
import { toast } from "@/lib/app/toast";
import { openSettings } from "@/features/settings/SettingsDialog";

const EMPTY_DRAFT: CommitDraft = { summary: "", body: "", coAuthors: [] };
const messageOf = (c: Commit): CommitDraft => ({ summary: c.subject, body: c.body, coAuthors: [] });

/**
 * The message being written, kept per worktree, and the Amend toggle that swaps in HEAD's.
 * `prepared`: git left a message for the next commit (status.preparedMessage).
 */
export function useCommitDraft(root: string, head: Commit | null, prepared: boolean) {
  const [draft, setDraft] = useState<CommitDraft>(() => loadDraft(root) ?? EMPTY_DRAFT);
  // While amending, the fields hold the message being amended (`original`, HEAD's at `sha`)
  // and the user's own draft waits aside.
  const [amend, setAmend] = useState<{ aside: CommitDraft; sha: string; original: CommitDraft } | null>(null);

  // An empty message starts as git's editor would: with the message a squash merge or
  // `cherry-pick -n` prepared, else commit.template in the description. An untouched one
  // follows as git writes or clears a prepared message.
  const [start, setStart] = useState<CommitDraft>(EMPTY_DRAFT);
  const started = useRef(start);
  useEffect(() => {
    let alive = true;
    api.commitTemplate().then(
      (t) => {
        if (!alive) return;
        const [summary = "", ...rest] = prepared && t ? t.split("\n") : [];
        const next = prepared ? { ...EMPTY_DRAFT, summary, body: rest.join("\n").trim() } : { ...EMPTY_DRAFT, body: t ?? "" };
        const old = started.current;
        started.current = next;
        setStart(next);
        setDraft((d) => (d.summary === old.summary && d.body === old.body ? { ...d, summary: next.summary, body: next.body } : d));
      },
      () => {},
    );
    return () => {
      alive = false;
    };
  }, [prepared]);

  // The draft outlives the panel (⌘2 and back, another worktree, a restart); an amend message isn't one.
  const keep = amend?.aside ?? draft;
  const kept = useRef(keep);
  useEffect(() => {
    kept.current = keep;
    const t = setTimeout(() => saveDraft(root, keep), 300);
    return () => clearTimeout(t);
  }, [root, keep]);
  // Leaving mid-debounce still keeps the last keystrokes.
  useEffect(() => () => saveDraft(root, kept.current), [root]);

  const edited = !!amend && (draft.summary !== amend.original.summary || draft.body !== amend.original.body);
  // An agent committing meanwhile moves HEAD; an untouched amend message follows it.
  useEffect(() => {
    if (!amend || !head || head.sha === amend.sha || edited) return;
    setAmend({ ...amend, sha: head.sha, original: messageOf(head) });
    setDraft(messageOf(head));
  }, [amend, head, edited]);

  const toggleAmend = (on: boolean) => {
    if (on && head) {
      setAmend({ aside: draft, sha: head.sha, original: messageOf(head) });
      setDraft(messageOf(head));
    } else if (!on && amend) {
      setDraft(amend.aside);
      setAmend(null);
    }
  };

  // After an amend, the draft set aside for it comes back.
  const clear = () => {
    setDraft(amend?.aside ?? start);
    setAmend(null);
  };

  // What the description starts as, for telling an untouched one from the user's.
  return { draft, setDraft, amend, edited, template: start.body, toggleAmend, clear };
}

/**
 * The configured agent CLI, run for a suggestion: `suggest(ask, land)` hands `ask` the command
 * line and `land` what it printed (false: nothing usable), unless `drop` came in between. Leaving
 * stops the command rather than orphan it. `what` names the suggestion in messages.
 */
export function useSuggestion(what: string) {
  const { suggestEnabled, suggestCommand, suggestModels } = useSettings();
  const [suggesting, setSuggesting] = useState(false);
  const running = useRef(false);
  useEffect(
    () => () => {
      if (running.current) api.suggestCancel().catch(() => {});
    },
    [],
  );
  const program = programOf(suggestCommand);
  const cancel = () => api.suggestCancel().catch(() => {});
  // A missing CLI or a stale model id is fixed there.
  const toSettings = { label: "Open Settings", run: () => openSettings("commit") };
  // Bumped by `drop`: a suggestion still on its way describes what was there before.
  const generation = useRef(0);
  const drop = () => {
    generation.current++;
    if (running.current) cancel();
  };
  const suggest = async (ask: (command: string) => Promise<string>, land: (output: string) => boolean) => {
    const gen = generation.current;
    running.current = true;
    setSuggesting(true);
    try {
      const output = await ask(commandLine(suggestCommand, suggestModels));
      if (gen === generation.current && !land(output)) toast("error", `No ${what} suggested`, `${program} printed nothing.`, toSettings);
    } catch (e) {
      if (e !== SUGGEST_CANCELLED && gen === generation.current) toast("error", `Couldn't suggest a ${what}`, errorMessage(e), toSettings);
    } finally {
      running.current = false;
      setSuggesting(false);
    }
  };
  return { enabled: suggestEnabled, suggesting, program, suggest, cancel, drop };
}

/** Asks the configured CLI for a commit message; `amend`/`hasStaged` pick which diff it describes. */
export function useSuggestMessage({
  draft,
  setDraft,
  template,
  amend,
  hasStaged,
  hasAny,
  busy,
}: {
  draft: CommitDraft;
  setDraft: (d: CommitDraft) => void;
  template: string | null;
  amend: boolean;
  hasStaged: boolean;
  hasAny: boolean;
  busy: boolean;
}) {
  const { enabled, suggesting, program, suggest: run, cancel, drop } = useSuggestion("message");
  const latest = useRef(draft);
  useEffect(() => {
    latest.current = draft;
  });
  const canSuggest = enabled && !suggesting && !busy && (amend || hasAny);
  const suggest = async () => {
    if (!canSuggest) return;
    await run(
      (command) => api.suggestMessage(command, SUGGEST_PROMPT, amend ? "amend" : hasStaged ? "staged" : "all"),
      (output) => {
        const message = parseSuggestion(output);
        if (!message) return false;
        const before = latest.current;
        setDraft({ ...before, ...message });
        // Never lost: what the user had comes back with one click.
        const blank = !before.summary.trim() && (!before.body.trim() || before.body === template);
        if (!blank) toast("info", "Message replaced with the suggestion", undefined, { label: "Restore", run: () => setDraft(before) });
        return true;
      },
    );
  };

  // A commit or an Amend toggle drops a suggestion still on its way: it would land in a fresh draft or the one set aside.
  return { suggesting, program, canSuggest, cancelSuggest: cancel, dropSuggestion: drop, suggest };
}
