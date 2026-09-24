// The code view's links (lib/links finds them, lib/linkHost resolves and opens them): ⌘-hover
// underlines one that resolves, ⌘-click opens it, as in VS Code; F12 or ⌘↵ opens the one at the cursor.
import { matchesCommand } from "./keybindings";
import { type LinkSide, linkKey, openTarget, resolverFor } from "./linkHost";
import { findLinks, type Target } from "./links";
import { monaco } from "./monaco";

/** A link on screen and where it goes. */
interface Found {
  range: monaco.Range;
  target: Target;
}

/** The link at `pos` in `lang` code, resolved (null: no link there, or one that goes nowhere). */
async function linkAt(model: monaco.editor.ITextModel, pos: monaco.IPosition, lang: string, side: LinkSide): Promise<Found | null> {
  // Ends included, as VS Code's links: the pointer's column is the caret spot nearest to it.
  const at = { start: pos.column - 1, end: pos.column - 1 };
  const link = findLinks(model.getLineContent(pos.lineNumber), lang, at).find((l) => pos.column >= l.start + 1 && pos.column <= l.end + 1);
  if (!link) return null;
  const target = (await resolverFor(side))(link);
  return target && { range: new monaco.Range(pos.lineNumber, link.start + 1, pos.lineNumber, link.end + 1), target };
}

/** Links in `editor`; `side` says what it shows now (null: no links). */
export function followLinks(editor: monaco.editor.ICodeEditor, lang: () => string, side: () => LinkSide | null): monaco.IDisposable {
  const underline = editor.createDecorationsCollection();
  let hovered: monaco.Position | null = null;
  let held = false;
  let shown: Found | null = null;
  let pressed: Found | null = null;
  // The link at the cursor, resolved as the cursor moves, so F12 / ⌘↵ know at once whether to take the key.
  let atCursor: Found | null = null;
  // Each lookup outdates the ones still on their way.
  let hoverSeq = 0;
  let cursorSeq = 0;

  const lookup = (pos: monaco.IPosition | null) => {
    const model = editor.getModel();
    const s = side();
    return model && pos && s ? linkAt(model, pos, lang(), s) : Promise.resolve(null);
  };
  const clear = () => {
    shown = null;
    underline.clear();
  };
  const update = () => {
    const id = ++hoverSeq;
    if (!held || !hovered) return clear();
    const pos = hovered;
    if (shown?.range.containsPosition(pos)) return;
    void lookup(pos).then((found) => {
      if (id !== hoverSeq) return;
      shown = found;
      if (found) underline.set([{ range: found.range, options: { inlineClassName: "gv-link" } }]);
      else underline.clear();
    });
  };
  const follow = () => {
    const id = ++cursorSeq;
    atCursor = null;
    void lookup(editor.getPosition()).then((found) => id === cursorSeq && (atCursor = found));
  };

  const onKey = (e: KeyboardEvent) => {
    if (linkKey(e) === held) return;
    held = linkKey(e);
    update();
  };
  const onBlur = () => {
    held = false;
    update();
  };
  window.addEventListener("keydown", onKey, true);
  window.addEventListener("keyup", onKey, true);
  window.addEventListener("blur", onBlur);

  const on = (pos: monaco.IPosition | null | undefined, found: Found | null): found is Found => !!pos && !!found && found.range.containsPosition(pos);
  const subs = [
    editor.onMouseMove((e) => {
      hovered = e.target.type === monaco.editor.MouseTargetType.CONTENT_TEXT ? e.target.position : null;
      held = linkKey(e.event);
      update();
    }),
    editor.onMouseLeave(() => {
      hovered = null;
      update();
    }),
    // Opens on release, and only if it's released on the link it was pressed on.
    editor.onMouseDown((e) => {
      pressed = e.event.leftButton && linkKey(e.event) && on(e.target.position, shown) ? shown : null;
    }),
    editor.onMouseUp((e) => {
      const link = pressed;
      pressed = null;
      if (link && link === shown && on(e.target.position, link)) {
        clear();
        openTarget(link.target);
      }
    }),
    editor.onDidChangeModel(() => {
      hovered = null;
      update();
      follow();
    }),
    editor.onDidChangeCursorPosition(follow),
    editor.onKeyDown((e) => {
      const link = atCursor;
      if (!matchesCommand("editor.openLink", e.browserEvent) || !on(editor.getPosition(), link)) return;
      e.preventDefault();
      e.stopPropagation();
      openTarget(link.target);
    }),
  ];

  return {
    dispose() {
      hoverSeq++;
      cursorSeq++;
      subs.forEach((s) => s.dispose());
      underline.clear();
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("keyup", onKey, true);
      window.removeEventListener("blur", onBlur);
    },
  };
}
