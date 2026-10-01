// Lines' comments in Monaco, a PR's and the review notes alike: drawn under a line in a view zone.
import { createRoot } from "react-dom/client";
import type { DiffRow } from "@/lib/api";
import type { monaco } from "@/lib/editor/monaco";

/** Unified view has no old side: where old line `line` goes there, under the new line before it. */
export function newLineBefore(rows: DiffRow[], line: number) {
  let before = 0;
  for (const r of rows) {
    if (r.o === line) return r.k === 0 ? r.n : before;
    if (r.n) before = r.n;
  }
  return before;
}

/**
 * `content` under line `after` of `code`, returning what takes it away. As Monaco's own zone
 * widgets: the view zone only makes room (its layer is under the text, out of reach of clicks),
 * and the content is an overlay widget kept on top of it, a small React root sized to what it holds.
 */
export function zoneWidget(code: monaco.editor.ICodeEditor, after: number, content: React.ReactNode) {
  const node = document.createElement("div");
  node.style.position = "absolute";
  // Keys typed in a comment are the comment's, not the editor's (⌘ chords still reach the app).
  node.addEventListener("keydown", (e) => !e.metaKey && !e.ctrlKey && e.stopPropagation());
  const root = createRoot(node);
  root.render(content);
  const zone: monaco.editor.IViewZone = {
    afterLineNumber: after,
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
  return () => {
    observer.disconnect();
    layout.dispose();
    root.unmount();
    code.removeOverlayWidget(widget);
    code.changeViewZones((a) => a.removeZone(id));
  };
}
