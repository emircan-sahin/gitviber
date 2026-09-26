import type { monaco } from "@/lib/editor/monaco";
import { codeFontFamily, type Settings } from "@/lib/settings";
import { BLAME_CHARS } from "./decorations";

export type CodeMode = "unified" | "split" | "file";

export const CONTEXT = 3;

function common(s: Settings, wrap: boolean): monaco.editor.IEditorOptions & monaco.editor.IGlobalEditorOptions {
  return {
    readOnly: true,
    automaticLayout: true,
    fontFamily: codeFontFamily(s),
    fontSize: s.codeFontSize,
    fontWeight: String(s.codeFontWeight),
    lineHeight: Math.round(s.codeFontSize * s.lineHeight),
    fontLigatures: s.ligatures,
    wordWrap: wrap ? "on" : "off",
    wrappingIndent: "same",
    // Plain text only: the HTML copy would carry the widened indentation.
    copyWithSyntaxHighlighting: false,
    minimap: { enabled: false },
    scrollBeyondLastLine: false,
    renderLineHighlight: "none",
    folding: false,
    glyphMargin: false,
    stickyScroll: { enabled: false },
    guides: { indentation: false },
    overviewRulerBorder: false,
    scrollbar: { useShadows: false, verticalScrollbarSize: 14, horizontalScrollbarSize: 10 },
    padding: { top: 4 },
  };
}

export function diffOptions(s: Settings, mode: CodeMode, collapse: boolean, wrap: boolean): monaco.editor.IDiffEditorConstructionOptions {
  return {
    ...common(s, wrap),
    renderSideBySide: mode === "split",
    // The layout is the user's choice, not the window width's.
    useInlineViewWhenSpaceIsLimited: false,
    hideUnchangedRegions: { enabled: collapse, contextLineCount: CONTEXT, minimumLineCount: 3, revealLineCount: 20 },
    // Whitespace changes are changes, as git counts them.
    ignoreTrimWhitespace: false,
    originalEditable: false,
    renderMarginRevertIcon: false,
    renderGutterMenu: false,
    diffWordWrap: "inherit",
    // A word change on one side only has an empty range on the other (see lib/git/diffHunks): no marker.
    experimental: { showEmptyDecorations: false },
    // Room around the +/− signs, like the old gutter's sign column.
    lineDecorationsWidth: 20,
    scrollbar: { ...common(s, wrap).scrollbar, vertical: "hidden", verticalScrollbarSize: 0 },
  };
}

export function fileOptions(s: Settings, wrap: boolean, blame: boolean, editable: boolean): monaco.editor.IStandaloneEditorConstructionOptions {
  // Blame's label goes after the change bars (index.css), in the code font's widths.
  return { ...common(s, wrap), readOnly: !editable, lineDecorationsWidth: blame ? `${BLAME_CHARS + 3}ch` : 12 };
}
