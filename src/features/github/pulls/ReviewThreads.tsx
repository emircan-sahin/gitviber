// A PR's line comments in its file diffs, as on GitHub: each thread under the line it's on, with
import { RelativeTime } from "@/components/RelativeTime";
// a reply box, and "Add Comment on Line" in the context menu for a new one. The threads live in
// Monaco view zones, each a small React root sized to what it holds.
import { useCallback, useMemo, useState } from "react";
import { type DiffRow, github, repoOf, type ReviewComment } from "@/lib/api";
import { useGitHubData } from "@/lib/github/githubCache";
import type { monaco } from "@/lib/editor/monaco";
import { PullMarkdown } from "@/features/github/shared/GitHubMarkdown";
import { Composer } from "@/features/review/Composer";
import { newLineBefore, zoneWidget } from "@/features/review/zones";

type Side = "LEFT" | "RIGHT";

/** A PR file's comments and how to add one. */
export interface Review {
  pull: { url: string; number: number };
  /** This file's comments, threads and replies alike. */
  comments: ReviewComment[];
  post: (side: Side, line: number, body: string, replyTo: number | null) => Promise<void>;
}

/** A PR file tab's comments (`pullUrl`: the PR; none for a comparison), kept fresh a minute at a time. */
export function useReview(pullUrl: string | undefined, number: number | undefined, head: string, path: string): Review | null {
  const target = pullUrl ? repoOf(pullUrl) : null;
  const key = pullUrl && number ? `review:${pullUrl}` : null;
  const fetch = useCallback(() => github.reviewComments(target, number ?? 0), [target, number]);
  const { data, refresh } = useGitHubData(key, fetch, 60_000);
  return useMemo(() => {
    if (!pullUrl || !number || !data) return null;
    return {
      pull: { url: pullUrl, number },
      comments: data.filter((c) => c.path === path),
      post: async (side, line, body, replyTo) => {
        await github.commentLine(target, number, head, path, line, side, body, replyTo);
        await refresh(true);
      },
    };
  }, [pullUrl, number, data, path, head, target, refresh]);
}

interface Shown {
  review: Review;
  rows: DiffRow[];
  unified: boolean;
}

/** A thread's place: which side's editor, after which of its lines. */
interface Spot {
  editor: "original" | "modified";
  after: number;
}

/** The threads of `get()` in `diff`; `update` redraws them (new comments, another file, the other layout). */
export function followReviewThreads(diff: monaco.editor.IStandaloneDiffEditor, get: () => Shown | null) {
  const editors = { original: diff.getOriginalEditor(), modified: diff.getModifiedEditor() };
  let zones: (() => void)[] = [];
  // A new comment being written, where.
  let draft: { side: Side; line: number } | null = null;

  const clear = () => {
    zones.forEach((dispose) => dispose());
    zones = [];
  };

  const spot = (side: Side, line: number, s: Shown): Spot => {
    if (side === "RIGHT") return { editor: "modified", after: line };
    if (!s.unified) return { editor: "original", after: line };
    return { editor: "modified", after: newLineBefore(s.rows, line) };
  };

  const add = (where: Spot, content: React.ReactNode) => void zones.push(zoneWidget(editors[where.editor], where.after, content));

  const update = () => {
    clear();
    const s = get();
    if (!s) return;
    const replies = new Map<number, ReviewComment[]>();
    for (const c of s.review.comments) if (c.replyTo) replies.set(c.replyTo, [...(replies.get(c.replyTo) ?? []), c]);
    for (const root of s.review.comments) {
      if (root.replyTo || root.line === null) continue;
      const thread = [root, ...(replies.get(root.id) ?? [])];
      add(spot(root.side, root.line, s), <Thread review={s.review} thread={thread} />);
    }
    if (draft) {
      const { side, line } = draft;
      add(
        spot(side, line, s),
        <Composer
          pull={s.review.pull}
          label={`Comment on line ${line}${side === "LEFT" ? " (old)" : ""}`}
          onCancel={() => {
            draft = null;
            update();
          }}
          onSubmit={async (body) => {
            await s.review.post(side, line, body, null);
            draft = null;
          }}
        />,
      );
    }
  };

  // "Add Comment on Line", in each side's editor.
  const subs = (["original", "modified"] as const).map((which) => {
    const code = editors[which];
    const has = code.createContextKey<boolean>("gitviberReview", false);
    const sync = () => has.set(!!get());
    const listeners = [code.onDidChangeModel(sync), code.onDidFocusEditorText(sync)];
    sync();
    const action = code.addAction({
      id: "gitviber.commentLine",
      label: "Add Comment on Line…",
      contextMenuGroupId: "0_review",
      precondition: "gitviberReview",
      run: () => {
        const line = code.getPosition()?.lineNumber;
        if (!line || !get()) return;
        draft = { side: which === "modified" ? "RIGHT" : "LEFT", line };
        update();
      },
    });
    return { dispose: () => [action, ...listeners].forEach((l) => l.dispose()) };
  });

  return {
    update,
    dispose() {
      clear();
      subs.forEach((s) => s.dispose());
    },
  };
}

function Thread({ review, thread }: { review: Review; thread: ReviewComment[] }) {
  const [replying, setReplying] = useState(false);
  const root = thread[0];
  return (
    <div className="mx-3 my-1.5 overflow-hidden rounded-md border border-border-strong bg-panel font-sans text-[12px] select-text">
      {thread.map((c) => (
        <div key={c.id} className="border-b border-border last:border-0">
          <div className="flex items-center gap-1.5 px-3 pt-2 text-[11.5px]">
            <span className="font-medium text-foreground">{c.author}</span>
            <RelativeTime date={c.createdAt} className="text-subtle" />
            {c === root && root.side === "LEFT" && <span className="ml-auto text-[10.5px] text-subtle">on removed line {root.line}</span>}
          </div>
          <PullMarkdown pull={review.pull} idPrefix={`rc-${c.id}-`} text={c.body} className="pt-1" />
        </div>
      ))}
      <div className="border-t border-border px-3 py-2">
        {replying ? (
          <Composer pull={review.pull} label="Reply" onCancel={() => setReplying(false)} onSubmit={(body) => review.post(root.side, root.line ?? 0, body, root.id)} bare />
        ) : (
          <button className="w-full rounded-sm border border-border bg-background px-2 py-1 text-left text-[11.5px] text-subtle hover:border-border-strong" onClick={() => setReplying(true)}>
            Reply…
          </button>
        )}
      </div>
    </div>
  );
}
