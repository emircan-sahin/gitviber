import { Code2, Copy, Workflow } from "lucide-react";
import { useEffect, useState } from "react";
import { Tip } from "@/components/ui/tooltip";
import { errorMessage } from "@/lib/api";
import { copyText } from "@/lib/app/clipboard";
import { useSettings } from "@/lib/settings";

// Rendered diagrams by theme and source: a tab switch or a re-render shows them at once.
const drawn = new Map<string, string>();
let queue: Promise<unknown> = Promise.resolve();

/** A diagram already drawn in this theme, to show at once; undefined until it is. */
export const drawnSvg = (code: string, dark: boolean) => drawn.get(`${dark}\0${code}`);
let ids = 0;

/**
 * Draws one diagram as SVG. Mermaid is loaded on the first one, and its settings are global, so
 * diagrams draw one at a time, each with the theme it asked for. `strict` runs no click handlers
 * and sanitizes labels (as GitHub and VS Code's preview do).
 */
export function draw(code: string, dark: boolean): Promise<string> {
  const key = `${dark}\0${code}`;
  const hit = drawn.get(key);
  if (hit) return Promise.resolve(hit);
  const run = queue.then(async () => {
    const { default: mermaid } = await import("mermaid");
    const fontFamily = getComputedStyle(document.documentElement).getPropertyValue("--font-ui").trim() || undefined;
    mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: dark ? "dark" : "default", suppressErrorRendering: true, fontFamily });
    const id = `mermaid-${++ids}`;
    try {
      const { svg } = await mermaid.render(id, code);
      drawn.set(key, svg);
      if (drawn.size > 64) drawn.delete(drawn.keys().next().value!);
      return svg;
    } finally {
      // A failed render can leave its scratch element behind.
      document.getElementById(`d${id}`)?.remove();
    }
  });
  queue = run.catch(() => {});
  return run;
}

/** A ```mermaid block: the diagram, or what's wrong with it and its source; the source on request. */
export function Mermaid({ code, fallback }: { code: string; fallback: React.ReactNode }) {
  const { dark } = useSettings();
  const [state, setState] = useState<{ key: string; svg?: string; error?: string } | null>(() => {
    const svg = drawnSvg(code, dark);
    return svg ? { key: `${dark}\0${code}`, svg } : null;
  });
  const [source, setSource] = useState(false);
  const key = `${dark}\0${code}`;
  useEffect(() => {
    let live = true;
    draw(code, dark).then(
      (svg) => live && setState({ key, svg }),
      (e) => live && setState({ key, error: errorMessage(e) }),
    );
    return () => {
      live = false;
    };
  }, [code, dark, key]);
  // The last diagram stays while a theme change redraws it.
  const shown = state && (state.key === key || state.svg) ? state : null;

  if (shown?.error)
    return (
      <div className="mermaid-diagram mermaid-error">
        <div className="mb-2 text-[12px] text-destructive select-text">Could not render this diagram: {shown.error}</div>
        {fallback}
      </div>
    );
  return (
    <div className="group/diagram relative">
      <div className="absolute top-1.5 right-1.5 z-10 flex gap-0.5 opacity-0 group-focus-within/diagram:opacity-100 group-hover/diagram:opacity-100">
        <DiagramButton label={source ? "Show diagram" : "Show source"} onClick={() => setSource(!source)}>
          {source ? <Workflow /> : <Code2 />}
        </DiagramButton>
        <DiagramButton label="Copy source" onClick={() => copyText(code, "Diagram source copied")}>
          <Copy />
        </DiagramButton>
      </div>
      {source ? (
        fallback
      ) : shown?.svg ? (
        <div
          className="mermaid-diagram"
          role="img"
          aria-label="Mermaid diagram"
          // Mermaid's own output under securityLevel "strict": sanitized with DOMPurify, no handlers.
          dangerouslySetInnerHTML={{ __html: shown.svg }}
        />
      ) : (
        <div className="mermaid-diagram text-[12px] text-subtle">Drawing diagram…</div>
      )}
    </div>
  );
}

function DiagramButton({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <Tip label={label}>
      <button
        aria-label={label}
        onClick={onClick}
        className="flex size-6 items-center justify-center rounded-sm border border-border bg-elevated text-subtle hover:text-foreground focus-visible:text-foreground [&_svg]:size-3.5"
      >
        {children}
      </button>
    </Tip>
  );
}
