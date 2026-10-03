import { useEffect, useState } from "react";

type Katex = typeof import("katex").default;

// KaTeX and its stylesheet (fonts bundled with the app, so it works offline) load with the first formula.
let katex: Katex | null = null;
let loading: Promise<Katex> | null = null;
const load = () => (loading ??= Promise.all([import("katex"), import("katex/dist/katex.min.css")]).then(([m]) => (katex = m.default)));

// Built with `trust` off, so \href, \url and \htmlClass and the like render as text, never links or markup.
const render = (k: Katex, tex: string, display: boolean) => k.renderToString(tex, { displayMode: display, throwOnError: false, strict: "ignore", output: "htmlAndMathml" });

/** $inline$ or $$display$$ math, as KaTeX draws it; the TeX itself until KaTeX has loaded. */
export function MathTex({ tex, display }: { tex: string; display: boolean }) {
  const [html, setHtml] = useState(() => katex && render(katex, tex, display));
  useEffect(() => {
    let live = true;
    void load().then((k) => live && setHtml(render(k, tex, display)));
    return () => {
      live = false;
    };
  }, [tex, display]);
  const Tag = display ? "div" : "span";
  if (!html) return <Tag className={display ? "math-display" : undefined}>{display ? <pre>{tex}</pre> : <code>{tex}</code>}</Tag>;
  return <Tag className={display ? "math-display" : "math-inline"} dangerouslySetInnerHTML={{ __html: html }} />;
}
