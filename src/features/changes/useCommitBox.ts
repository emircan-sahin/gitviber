import { useEffect, useRef, useState } from "react";
import { api, type Commit, errorMessage, SUGGEST_CANCELLED } from "@/lib/api";
import { type CommitDraft, loadDraft, saveDraft } from "@/lib/repo/session";
import { useSettings } from "@/lib/settings";
import { commandLine, parseSuggestion, programOf, SUGGEST_PROMPT } from "@/lib/git/suggest";
import { toast } from "@/lib/app/toast";
import { openSettings } from "@/features/settings/SettingsDialog";

const EMPTY_DRAFT: CommitDraft = { summary: "", body: "", coAuthors: [] };
const messageOf = (c: Commit): CommitDraft => ({ summary: c.subject, body: c.body, coAuthors: [] });

/** The message being written, kept per worktree, and the Amend toggle that swaps in HEAD's. */
export function useCommitDraft(root: string, head: Commit | null) {
  const [draft, setDraft] = useState<CommitDraft>(() => loadDraft(root) ?? EMPTY_DRAFT);
  // While amending, the fields hold the message being amended (`original`, HEAD's at `sha`)
  // and the user's own draft waits aside.
  const [amend, setAmend] = useState<{ aside: CommitDraft; sha: string; original: CommitDraft } | null>(null);

  // An empty message starts from commit.template, as git's editor would.
  const [template, setTemplate] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    api.commitTemplate().then(
      (t) => {
        if (!alive || !t) return;
        setTemplate(t);
        setDraft((d) => (d.summary || d.body ? d : { ...d, body: t }));
      },
      () => {},
    );
    return () => {
      alive = false;
    };
  }, []);

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
    setDraft(amend?.aside ?? { ...EMPTY_DRAFT, body: template ?? "" });
    setAmend(null);
  };

  return { draft, setDraft, amend, edited, template, toggleAmend, clear };
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
  const { suggestEnabled, suggestCommand, suggestModels } = useSettings();
  const [suggesting, setSuggesting] = useState(false);
  const running = useRef(false);
  const latest = useRef(draft);
  useEffect(() => {
    latest.current = draft;
  });
  // Leaving the box (the History tab, another worktree) stops the command rather than orphan it.
  useEffect(
    () => () => {
      if (running.current) api.suggestCancel().catch(() => {});
    },
    [],
  );
  const program = programOf(suggestCommand);
  const canSuggest = suggestEnabled && !suggesting && !busy && (amend || hasAny);
  const cancelSuggest = () => api.suggestCancel().catch(() => {});
  // A missing CLI or a stale model id is fixed there.
  const toSettings = { label: "Open Settings", run: () => openSettings("commit") };
  // Bumped by a commit or an Amend toggle: a suggestion still on its way describes the diff
  // before them, and would land in a fresh draft or the one set aside.
  const generation = useRef(0);
  const dropSuggestion = () => {
    generation.current++;
    if (running.current) cancelSuggest();
  };
  const suggest = async () => {
    if (!canSuggest) return;
    const gen = generation.current;
    running.current = true;
    setSuggesting(true);
    try {
      const output = await api.suggestMessage(commandLine(suggestCommand, suggestModels), SUGGEST_PROMPT, amend ? "amend" : hasStaged ? "staged" : "all");
      if (gen !== generation.current) return;
      const message = parseSuggestion(output);
      if (!message) {
        toast("error", "No message suggested", `${program} printed nothing.`, toSettings);
        return;
      }
      const before = latest.current;
      setDraft({ ...before, ...message });
      // Never lost: what the user had comes back with one click.
      const blank = !before.summary.trim() && (!before.body.trim() || before.body === template);
      if (!blank) toast("info", "Message replaced with the suggestion", undefined, { label: "Restore", run: () => setDraft(before) });
    } catch (e) {
      if (e !== SUGGEST_CANCELLED && gen === generation.current) toast("error", "Couldn't suggest a message", errorMessage(e), toSettings);
    } finally {
      running.current = false;
      setSuggesting(false);
    }
  };

  return { suggesting, program, canSuggest, cancelSuggest, dropSuggestion, suggest };
}
