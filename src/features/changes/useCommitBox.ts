import { useEffect, useRef, useState } from "react";
import { api, type Commit, errorMessage, SUGGEST_CANCELLED, type SuggestKind } from "@/lib/api";
import { type CommitDraft, loadDraft, saveDraft } from "@/lib/repo/session";
import { useSettings } from "@/lib/settings";
import { commandLine, parseSuggestion, programOf, SUGGEST_PROMPT } from "@/lib/git/suggest";
import { toast } from "@/lib/app/toast";
import { openSettings } from "@/features/settings/SettingsDialog";

const EMPTY_DRAFT: CommitDraft = { summary: "", body: "", coAuthors: [] };
const messageOf = (c: Commit): CommitDraft => ({ summary: c.subject, body: c.body, coAuthors: [] });
type Message = { summary: string; body: string };
const NO_MESSAGE: Message = { summary: "", body: "" };
const same = (d: CommitDraft, m: Message) => d.summary === m.summary && d.body === m.body;
/** A draft nobody wrote in: blank, or still the message it started from. */
const untouched = (d: CommitDraft) => (!d.summary.trim() && !d.body.trim()) || same(d, d.from ?? NO_MESSAGE);
const startingFrom = (d: CommitDraft, from: Message): CommitDraft => ({ ...d, ...from, from });

/**
 * The message being written, kept per worktree, and the Amend toggle that swaps in HEAD's.
 * `prepared`: status.preparedMessage, set while git has left a message for the next commit and
 * changing with it.
 */
export function useCommitDraft(root: string, head: Commit | null, prepared: string | null) {
  const [draft, setDraft] = useState<CommitDraft>(() => loadDraft(root) ?? EMPTY_DRAFT);
  // While amending, the fields hold the message being amended (`original`, HEAD's at `sha`)
  // and the user's own draft waits aside.
  const [amend, setAmend] = useState<{ aside: CommitDraft; sha: string; original: CommitDraft } | null>(null);

  // An empty message starts as git's editor would: with the message a squash merge or
  // `cherry-pick -n` prepared, else commit.template in the description. A draft still as it
  // started (kept with it, across remounts) follows each new prepared message and its clearing.
  const [start, setStart] = useState<Message>(NO_MESSAGE);
  const started = useRef(start);
  const amending = useRef(amend);
  amending.current = amend;
  useEffect(() => {
    let alive = true;
    api.commitTemplate().then(
      (t) => {
        if (!alive) return;
        const [summary = "", ...rest] = prepared && t ? t.split("\n") : [];
        const next = prepared ? { summary, body: rest.join("\n").trim() } : { summary: "", body: t ?? "" };
        const old = started.current;
        started.current = next;
        setStart(next);
        // A draft saved before drafts kept their start is untouched while it equals one.
        const follow = (d: CommitDraft) => (untouched(d) || (!d.from && (same(d, old) || same(d, next))) ? startingFrom(d, next) : d);
        // The fields hold HEAD's message while amending; the draft set aside follows instead.
        if (!amending.current) setDraft(follow);
        setAmend((a) => (a && follow(a.aside) !== a.aside ? { ...a, aside: follow(a.aside) } : a));
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
    setDraft(amend?.aside ?? startingFrom(EMPTY_DRAFT, started.current));
    setAmend(null);
  };

  // What a new draft's description starts as, for telling an untouched one from the user's.
  return { draft, setDraft, amend, edited, startBody: start.body, toggleAmend, clear };
}

/**
 * The configured agent CLI, run for a suggestion: `suggest(ask, land)` hands `ask` the command
 * line and `land` what it printed (false: nothing usable), unless `drop` came in between. Leaving
 * stops the command rather than orphan it. `kind`: the commit box's, or the pull request dialog's.
 */
export function useSuggestion(kind: SuggestKind) {
  const what = kind === "pull" ? "description" : "message";
  const { suggestEnabled, suggestCommand, suggestModels } = useSettings();
  const [suggesting, setSuggesting] = useState(false);
  const running = useRef(false);
  useEffect(
    () => () => {
      if (running.current) api.suggestCancel(kind).catch(() => {});
    },
    [],
  );
  const program = programOf(suggestCommand);
  const cancel = () => api.suggestCancel(kind).catch(() => {});
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
  startBody,
  amend,
  hasStaged,
  hasAny,
  busy,
}: {
  draft: CommitDraft;
  setDraft: (d: CommitDraft) => void;
  startBody: string;
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
        const blank = !before.summary.trim() && (!before.body.trim() || before.body === startBody);
        if (!blank) toast("info", "Message replaced with the suggestion", undefined, { label: "Restore", run: () => setDraft(before) });
        return true;
      },
    );
  };

  // A commit or an Amend toggle drops a suggestion still on its way: it would land in a fresh draft or the one set aside.
  return { suggesting, program, canSuggest, cancelSuggest: cancel, dropSuggestion: drop, suggest };
}
