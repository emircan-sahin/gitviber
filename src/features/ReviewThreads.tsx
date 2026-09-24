// A PR's line comments in its file diffs, as on GitHub: each thread under the line it's on, with
// a reply box, and "Add Comment on Line" in the context menu for a new one. The threads live in
// Monaco view zones, each a small React root sized to what it holds.
import { useCallback, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { type DiffRow, errorMessage, github, repoOf, type ReviewComment } from "@/lib/api";
import { useGitHubData } from "@/lib/githubCache";
import { monaco } from "@/lib/monaco";
import { toast } from "@/lib/toast";
import { relativeTime } from "@/lib/utils";
import { isoToUnix } from "./PullsPanel";
import { PullMarkdown } from "./PullView";

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
  let zones: { editor: "original" | "modified"; id: string; dispose: () => void }[] = [];
  // A new comment being written, where.
  let draft: { side: Side; line: number } | null = null;

  const clear = () => {
    zones.forEach((z) => z.dispose());
    zones = [];
  };

  // Unified view has no old side: a comment on a removed line goes under the new line before it.
  const spot = (side: Side, line: number, s: Shown): Spot => {
    if (side === "RIGHT") return { editor: "modified", after: line };
    if (!s.unified) return { editor: "original", after: line };
    let before = 0;
    for (const r of s.rows) {
      if (r.o === line) return { editor: "modified", after: r.k === 0 ? r.n : before };
      if (r.n) before = r.n;
    }
    return { editor: "modified", after: before };
  };

  // As Monaco's own zone widgets: the view zone only makes room (its layer is under the text,
  // out of reach of clicks), and the thread is an overlay widget kept on top of it.
  const add = (where: Spot, content: React.ReactNode) => {
    const code = editors[where.editor];
    const node = document.createElement("div");
    node.style.position = "absolute";
    // Keys typed in a comment are the comment's, not the editor's (⌘ chords still reach the app).
    node.addEventListener("keydown", (e) => !e.metaKey && !e.ctrlKey && e.stopPropagation());
    const root = createRoot(node);
    root.render(content);
    const zone: monaco.editor.IViewZone = {
      afterLineNumber: where.after,
      // Right under its line, before the diff's own zones there (removed lines, which default to
      // 10000): after them, unified view's old line numbers came loose from their lines.
      ordinal: 0,
      heightInPx: 60,
      domNode: document.createElement("div"),
      onDomNodeTop: (top) => {
        node.style.top = `${top}px`;
      },
    };
    let id = "";
    code.changeViewZones((a) => {
      id = a.addZone(zone);
    });
    const widget: monaco.editor.IOverlayWidget = { getId: () => `gitviber.review.${id}`, getDomNode: () => node, getPosition: () => null };
    code.addOverlayWidget(widget);
    const place = () => {
      const info = code.getLayoutInfo();
      node.style.left = `${info.contentLeft}px`;
      node.style.width = `${Math.max(0, info.contentWidth - info.verticalScrollbarWidth)}px`;
    };
    place();
    const layout = code.onDidLayoutChange(place);
    // The room it takes follows what it holds, as that loads and grows.
    const observer = new ResizeObserver(() => {
      const height = node.offsetHeight;
      if (!height || Math.abs(height - (zone.heightInPx ?? 0)) < 1) return;
      zone.heightInPx = height;
      code.changeViewZones((a) => a.layoutZone(id));
    });
    observer.observe(node);
    zones.push({
      editor: where.editor,
      id,
      dispose: () => {
        observer.disconnect();
        layout.dispose();
        root.unmount();
        code.removeOverlayWidget(widget);
        code.changeViewZones((a) => a.removeZone(id));
      },
    });
  };

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
            <span className="text-subtle">{relativeTime(isoToUnix(c.createdAt))}</span>
            {c === root && root.side === "LEFT" && <span className="ml-auto text-[10.5px] text-subtle">on removed line {root.line}</span>}
          </div>
          <PullMarkdown pull={review.pull} idPrefix={`rc-${c.id}-`} text={c.body} className="pt-1" />
        </div>
      ))}
      <div className="border-t border-border px-3 py-2">
        {replying ? (
          <Composer label="Reply" onCancel={() => setReplying(false)} onSubmit={(body) => review.post(root.side, root.line ?? 0, body, root.id)} bare />
        ) : (
          <button className="w-full rounded-sm border border-border bg-background px-2 py-1 text-left text-[11.5px] text-subtle hover:border-border-strong" onClick={() => setReplying(true)}>
            Reply…
          </button>
        )}
      </div>
    </div>
  );
}

function Composer({ label, onSubmit, onCancel, bare = false }: { label: string; onSubmit: (body: string) => Promise<void>; onCancel: () => void; bare?: boolean }) {
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    if (!body.trim() || busy) return;
    setBusy(true);
    try {
      await onSubmit(body);
      setBody("");
      onCancel();
    } catch (e) {
      toast("error", "Could not post the comment", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const box = (
    <div className="flex flex-col gap-2">
      <Textarea
        autoFocus
        value={body}
        onChange={(e) => setBody(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            e.stopPropagation();
            void submit();
          } else if (e.key === "Escape") onCancel();
        }}
        rows={3}
        placeholder={`${label}… (Markdown; ⌘↵ to post)`}
      />
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
        <Button size="sm" disabled={!body.trim() || busy} onClick={() => void submit()}>
          {busy ? "Posting…" : "Comment"}
        </Button>
      </div>
    </div>
  );
  return bare ? box : <div className="mx-3 my-1.5 rounded-md border border-border-strong bg-panel p-2 font-sans">{box}</div>;
}
