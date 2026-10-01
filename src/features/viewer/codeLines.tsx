// Code drawn as HTML rather than in Monaco (the conflict view, the stacked diff), looking as Monaco draws it.
import type { TokenLine } from "@/lib/editor/highlight";
import { TAB } from "@/lib/editor/indent";
import { emphasized } from "@/lib/git/diffHunks";
import { codeFontFamily, useSettings } from "@/lib/settings";

/** The code view's font, line height, tab width and ligatures. */
export function useCodeStyle() {
  const s = useSettings();
  return {
    fontFamily: codeFontFamily(s),
    fontSize: s.codeFontSize,
    fontWeight: s.codeFontWeight,
    lineHeight: `${Math.round(s.codeFontSize * s.lineHeight)}px`,
    tabSize: TAB,
    fontVariantLigatures: s.ligatures ? "normal" : "none",
  } as const;
}

/** A line's text in its highlighted colors (none yet: plain); word changes (`ranges`) get the `emphasis` class. */
export function Tokens({ text, tokens, ranges = [], emphasis }: { text: string; tokens?: TokenLine; ranges?: [number, number][]; emphasis?: string }) {
  if (!tokens && !ranges.length) return <>{text}</>;
  return (
    <>
      {emphasized(text, tokens, ranges).map(([t, color, fs, emph], i) => (
        <span key={i} className={emph ? emphasis : undefined} style={{ color: color || undefined, fontStyle: fs & 1 ? "italic" : undefined, fontWeight: fs & 2 ? 600 : undefined }}>
          {t}
        </span>
      ))}
    </>
  );
}
