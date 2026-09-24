// Staging part of a file from its diff, as in VS Code: hovering a change shows Stage and Discard
// (Unstage on the staged side), and the context menu stages, unstages or discards the selected
// lines, or the change at the cursor. lib/lineStaging says which lines; lines.rs writes them.
import { api, type DiffPair, errorMessage } from "./api";
import { type Change, changeAt, changes, isEmpty, type Picked, pick, type Side, whole } from "./lineStaging";
import { monaco } from "./monaco";
import { toast } from "./toast";
import { tracked, undoAction } from "./undo";

/** A working-tree diff whose lines can move: index → worktree ("unstaged") or HEAD → index ("staged"). */
export interface Staging {
  kind: "unstaged" | "staged";
  path: string;
  pair: DiffPair;
  refresh: () => unknown;
}

type Action = "stage" | "unstage" | "discard";

const LABELS: Record<Action, [change: string, lines: string, failed: string]> = {
  stage: ["Stage Change", "Stage Selected Lines", "Stage failed"],
  unstage: ["Unstage Change", "Unstage Selected Lines", "Unstage failed"],
  discard: ["Discard Change", "Discard Selected Lines", "Discard failed"],
};

const actionsFor = (kind: Staging["kind"]): Action[] => (kind === "unstaged" ? ["stage", "discard"] : ["unstage"]);

async function run(s: Staging, action: Action, p: Picked) {
  if (isEmpty(p)) return;
  const { pair } = s;
  const request = {
    path: s.path,
    kind: s.kind,
    action,
    original: pair.original.exists ? pair.original.text : null,
    modified: pair.modified.exists ? pair.modified.text : null,
    removed: p.removed,
    added: p.added,
  };
  try {
    if (action === "discard") {
      const [, entry] = await tracked(() => api.changeLines(request));
      toast("success", `Discarded lines in ${s.path}`, "The old version is in the Trash.", undoAction(entry, s.refresh));
    } else await api.changeLines(request);
  } catch (e) {
    toast("error", LABELS[action][2], errorMessage(e));
  }
  await s.refresh();
}

/** Stage, unstage and discard in `diff` for what `staging` says it shows (null: a diff that can't). */
export function followLineActions(diff: monaco.editor.IStandaloneDiffEditor, staging: () => Staging | null): monaco.IDisposable {
  const subs: monaco.IDisposable[] = [];
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
      if (!sel.isEmpty()) {
        // A selection that ends at the start of a line doesn't take that line.
        const to = sel.endColumn === 1 && sel.endLineNumber > sel.startLineNumber ? sel.endLineNumber - 1 : sel.endLineNumber;
        return pick(list(), side, sel.startLineNumber, to);
      }
      const c = changeAt(list(), side, sel.startLineNumber);
      return c && whole(c);
    };
    const update = () => {
      const s = staging();
      kind.set(s?.kind ?? "");
      onChange.set(!!s && !!changeAt(list(), side, code.getPosition()?.lineNumber ?? 0));
    };
    subs.push(code.onDidChangeCursorPosition(update), code.onDidChangeModel(update), code.onDidFocusEditorText(update));
    for (const action of ["stage", "unstage", "discard"] as Action[]) {
      const on = `gitviberStaging == ${action === "unstage" ? "staged" : "unstaged"}`;
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
    update();
  }

  subs.push(hoverBar(diff, staging, list));
  return { dispose: () => subs.forEach((s) => s.dispose()) };
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
    if (!c || !s) {
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
