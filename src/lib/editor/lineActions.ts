// Staging part of a file from its diff, as in VS Code: hovering a change shows Stage and Discard
// (Unstage on the staged side), and the context menu and the Diff commands' keys stage, unstage or
// discard the selected lines, or the change at the cursor. lib/git/lineStaging says which lines;
// lines.rs writes them. A commit's diff reverts lines in the working tree instead (revert.rs), and
// any of them copies the selected lines as a patch.
import { api, type DiffPair, errorMessage, type LinesRequest } from "../api";
import { type Change, changeAt, changes, isEmpty, type Picked, pick, type Side, whole } from "../git/lineStaging";
import { monaco } from "./monaco";
import { toast } from "../app/toast";
import { copyLater } from "../app/clipboard";
import { rewriteFiles } from "../repo/undo";

/**
 * Where a diff's lines can go: index → worktree ("unstaged") or HEAD → index ("staged") stage,
 * unstage and discard; a commit's ("commit") revert in the working tree.
 */
export type StagingSide = ({ kind: "unstaged" | "staged" } | { kind: "commit"; sha: string }) & {
  /** A staged rename's old path: HEAD's side of the diff is read there. */
  oldPath: string | null;
  refresh: () => unknown;
};

/** A diff on show whose lines can move. */
export type Staging = StagingSide & { path: string; pair: DiffPair };

export type LineAction = LinesRequest["action"] | "revert";

/** The lines a selection takes, first and last. One that ends at the start of a line doesn't take that line. */
export function selectedLines(sel: monaco.Selection): [number, number] {
  return [sel.startLineNumber, sel.endColumn === 1 && sel.endLineNumber > sel.startLineNumber ? sel.endLineNumber - 1 : sel.endLineNumber];
}

const LABELS: Record<LineAction, [change: string, lines: string, failed: string]> = {
  stage: ["Stage Change", "Stage Selected Lines", "Stage failed"],
  unstage: ["Unstage Change", "Unstage Selected Lines", "Unstage failed"],
  discard: ["Discard Change", "Discard Selected Lines", "Discard failed"],
  revert: ["Revert Change", "Revert Selected Lines", "Revert failed"],
};

// Which diff each action is on: its menu item's precondition, and the hover bar's buttons in this order.
const KIND_OF: Record<LineAction, Staging["kind"]> = { stage: "unstaged", discard: "unstaged", unstage: "staged", revert: "commit" };
const actionsFor = (kind: Staging["kind"]) => (Object.keys(KIND_OF) as LineAction[]).filter((a) => KIND_OF[a] === kind);

async function run(s: Staging, action: LineAction, p: Picked) {
  if (isEmpty(p) || KIND_OF[action] !== s.kind) return;
  if (s.kind === "commit") {
    const request = { sha: s.sha, path: s.path, oldPath: s.oldPath, removed: p.removed, added: p.added };
    return rewriteFiles(`Reverted lines in ${s.path}`, LABELS.revert[2], () => api.revertLines(request), s.refresh);
  }
  if (action === "revert") return;
  const { pair } = s;
  const request: LinesRequest = {
    path: s.path,
    oldPath: s.oldPath,
    kind: s.kind,
    action,
    original: pair.original.exists ? pair.original.text : null,
    modified: pair.modified.exists ? pair.modified.text : null,
    removed: p.removed,
    added: p.added,
  };
  if (action === "discard") return rewriteFiles(`Discarded lines in ${s.path}`, LABELS.discard[2], () => api.changeLines(request), s.refresh);
  try {
    await api.changeLines(request);
  } catch (e) {
    toast("error", LABELS[action][2], errorMessage(e));
  }
  await s.refresh();
}

export interface LineActions extends monaco.IDisposable {
  /** The keyboard's Stage / Unstage / Discard Change: the context menu's pick, if it's on screen. */
  act(action: LineAction): void;
}

/** Stage, unstage and discard in `diff` for what `staging` says it shows (null: a diff that can't). */
export function followLineActions(diff: monaco.editor.IStandaloneDiffEditor, staging: () => Staging | null): LineActions {
  const subs: monaco.IDisposable[] = [];
  const pickers = new Map<monaco.editor.ICodeEditor, () => Picked | null>();
  // The side the keys act on: where the cursor was last put in this file (a click, a selection, Next
  // Change). Split view has a cursor on each side, and restoring a file's view moves both.
  // Staged and unstaged share the editor, each with its own cursors.
  let last: { path: string; kind: Staging["kind"]; code: monaco.editor.ICodeEditor } | null = null;
  // Until the last one's refresh lands: the diff it read is stale by then.
  let busy = false;
  // Read on every pointer move: worked out once per diff.
  let memo: { rows: DiffPair["rows"]; list: Change[] } | null = null;
  const list = () => {
    const rows = staging()?.pair.rows;
    if (!rows) return [];
    if (memo?.rows !== rows) memo = { rows, list: changes(rows) };
    return memo.list;
  };

  // The context menu, in each side's editor: the selection there, else the change at the cursor.
  for (const [code, side] of [
    [diff.getOriginalEditor(), "original"],
    [diff.getModifiedEditor(), "modified"],
  ] as const) {
    const kind = code.createContextKey<string>("gitviberStaging", "");
    const onChange = code.createContextKey<boolean>("gitviberOnChange", false);
    const chosen = (): Picked | null => {
      const sel = code.getSelection();
      if (!sel) return null;
      if (!sel.isEmpty()) return pick(list(), side, ...selectedLines(sel));
      const c = changeAt(list(), side, sel.startLineNumber);
      return c && whole(c);
    };
    const update = () => {
      const s = staging();
      kind.set(s?.kind ?? "");
      onChange.set(!!s && !!changeAt(list(), side, code.getPosition()?.lineNumber ?? 0));
    };
    const moved = (e: monaco.editor.ICursorPositionChangedEvent) => {
      const s = staging();
      if (s && e.source !== "restoreState") last = { path: s.path, kind: s.kind, code };
      update();
    };
    pickers.set(code, chosen);
    subs.push(code.onDidChangeCursorPosition(moved), code.onDidChangeModel(update), code.onDidFocusEditorText(update));
    for (const action of Object.keys(LABELS) as LineAction[]) {
      const on = `gitviberStaging == ${KIND_OF[action]}`;
      const [change, lines] = LABELS[action];
      const go = () => {
        const s = staging();
        const p = s && chosen();
        if (s && p) void run(s, action, p);
      };
      subs.push(
        code.addAction({ id: `gitviber.${action}Change`, label: change, contextMenuGroupId: "0_staging", precondition: `${on} && !editorHasSelection && gitviberOnChange`, run: go }),
        code.addAction({ id: `gitviber.${action}Lines`, label: lines, contextMenuGroupId: "0_staging", precondition: `${on} && editorHasSelection`, run: go }),
      );
    }
    subs.push(
      code.addAction({
        id: "gitviber.copyLinesPatch",
        label: "Copy Selected Lines as Patch",
        contextMenuGroupId: "0_staging",
        precondition: "gitviberStaging != '' && editorHasSelection",
        run: () => {
          const s = staging();
          const p = s && chosen();
          if (!s || !p || isEmpty(p)) return;
          const { original, modified } = s.pair;
          const request = { path: s.path, oldPath: s.oldPath, original: original.exists ? original.text : null, modified: modified.exists ? modified.text : null, removed: p.removed, added: p.added };
          void copyLater(() => api.linesPatch(request), "Patch copied");
        },
      }),
    );
    update();
  }

  subs.push(hoverBar(diff, staging, list));
  return {
    act(action) {
      const s = staging();
      if (!s || busy || !actionsFor(s.kind).includes(action)) return;
      const code = last?.path === s.path && last.kind === s.kind ? last.code : diff.getModifiedEditor();
      const sel = code.getSelection();
      // The cursor stays put while the arrows scroll: a change scrolled out of sight isn't the one being read.
      const seen = !!sel && code.getVisibleRanges().some((r) => r.startLineNumber <= sel.endLineNumber && sel.startLineNumber <= r.endLineNumber);
      const p = seen ? pickers.get(code)?.() : null;
      if (p && !isEmpty(p)) {
        busy = true;
        void run(s, action, p).finally(() => (busy = false));
      } else toast("info", "No change at the cursor", "Click into a change, or go to one with Next Change.");
    },
    dispose: () => subs.forEach((s) => s.dispose()),
  };
}

/** Buttons over the change under the pointer, at its top right. */
function hoverBar(diff: monaco.editor.IStandaloneDiffEditor, staging: () => Staging | null, list: () => Change[]): monaco.IDisposable {
  const modified = diff.getModifiedEditor();
  const node = document.createElement("div");
  node.className = "gv-hunk-bar";
  node.style.display = "none";
  let shown: Change | null = null;
  let hideTimer = 0;
  const widget: monaco.editor.IOverlayWidget = {
    getId: () => "gitviber.hunkBar",
    getDomNode: () => node,
    getPosition: () => (shown ? { preference: place(shown) } : null),
  };
  // Pressing a button must not move the cursor or start a selection under it.
  node.addEventListener("mousedown", (e) => e.preventDefault());

  const place = (c: Change): monaco.editor.IOverlayWidgetPositionCoordinates => {
    const line = Math.min(c.modified[0], modified.getModel()?.getLineCount() ?? 1);
    // The top of the change: above its removed lines, which unified view draws before it.
    const top = modified.getTopForLineNumber(line, true) - modified.getScrollTop();
    const { width, verticalScrollbarWidth } = modified.getLayoutInfo();
    return { top: Math.max(0, top), left: width - verticalScrollbarWidth - node.offsetWidth - 24 };
  };

  const show = (c: Change | null) => {
    window.clearTimeout(hideTimer);
    const s = staging();
    // A commit's lines revert from the menu: browsing history shouldn't offer it on every change.
    if (!c || !s || s.kind === "commit") {
      shown = null;
      node.style.display = "none";
      return;
    }
    if (c !== shown || node.dataset.kind !== s.kind) {
      node.replaceChildren(
        ...actionsFor(s.kind).map((action) => {
          const b = document.createElement("button");
          b.textContent = action === "stage" ? "Stage" : action === "unstage" ? "Unstage" : "Discard";
          b.title = LABELS[action][0];
          b.className = action === "discard" ? "gv-hunk-discard" : "";
          b.onclick = () => {
            const now = staging();
            show(null);
            if (now) void run(now, action, whole(c));
          };
          return b;
        }),
      );
      node.dataset.kind = s.kind;
    }
    shown = c;
    node.style.display = "";
    modified.layoutOverlayWidget(widget);
  };
  const hideSoon = () => {
    window.clearTimeout(hideTimer);
    hideTimer = window.setTimeout(() => show(null), 300);
  };

  const onMove = (side: Side) => (e: monaco.editor.IEditorMouseEvent) => {
    const t = e.target;
    if (t.type === monaco.editor.MouseTargetType.OVERLAY_WIDGET) return window.clearTimeout(hideTimer);
    // Unified view's removed lines are a zone above the line they were before.
    const line = t.type === monaco.editor.MouseTargetType.CONTENT_VIEW_ZONE ? t.detail.afterLineNumber + 1 : t.position?.lineNumber;
    const c = line ? changeAt(list(), side, line) : null;
    if (c) show(c);
    else hideSoon();
  };
  modified.addOverlayWidget(widget);
  const subs = [
    modified.onMouseMove(onMove("modified")),
    diff.getOriginalEditor().onMouseMove(onMove("original")),
    modified.onMouseLeave(hideSoon),
    diff.getOriginalEditor().onMouseLeave(hideSoon),
    modified.onDidScrollChange(() => shown && modified.layoutOverlayWidget(widget)),
    modified.onDidLayoutChange(() => shown && modified.layoutOverlayWidget(widget)),
    modified.onDidChangeModel(() => show(null)),
  ];
  node.addEventListener("mouseleave", hideSoon);
  return {
    dispose() {
      window.clearTimeout(hideTimer);
      subs.forEach((s) => s.dispose());
      modified.removeOverlayWidget(widget);
    },
  };
}
