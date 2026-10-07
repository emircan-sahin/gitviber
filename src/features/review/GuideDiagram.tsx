import { type SyntheticEvent, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Tip } from "@/components/ui/tooltip";
import { errorMessage } from "@/lib/api";
import { describeFlow, type Flow, flowSource, type Model, type PartStatus, sectionBadge, statusWord } from "@/lib/review/guide";
import { useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { draw, drawnSvg } from "@/features/viewer/markdown/Mermaid";

interface Links {
  /** The sections' titles, for the names of the links to them. */
  titles: string[];
  /** Scrolls to section `n` (from 1). */
  onSection: (n: number) => void;
}

const sectionName = (n: number, titles: string[]) => `Section ${n}: ${titles[n - 1] || "Untitled"}`;

/** A guide's diagram: the data it changes as cards of their fields, then its flows. */
export function GuideDiagram({ models, flows, ...links }: { models: Model[]; flows: Flow[] } & Links) {
  return (
    <div className="flex flex-col gap-4">
      {models.length > 0 && (
        <div>
          <h3 className="mb-1.5 text-[12px] font-medium text-muted-foreground">Data</h3>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-2">
            {models.map((m, i) => (
              <ModelCard key={i} model={m} {...links} />
            ))}
          </div>
        </div>
      )}
      {flows.map((f, i) => (
        <FlowDiagram key={i} flow={f} {...links} />
      ))}
    </div>
  );
}

const MARKS: Record<PartStatus, { sign: string; className: string }> = {
  new: { sign: "+", className: "text-added" },
  changed: { sign: "~", className: "text-modified" },
  same: { sign: "", className: "" },
};

function Mark({ status }: { status: PartStatus }) {
  const { sign, className } = MARKS[status];
  return (
    <span className={cn("w-2.5 shrink-0 text-center font-mono font-semibold", className)}>
      <span aria-hidden>{sign}</span>
      {sign && <span className="sr-only">{statusWord(status)}</span>}
    </span>
  );
}

function Badge({ n, titles, onSection }: { n: number | null } & Links) {
  if (!n) return null;
  const name = sectionName(n, titles);
  return (
    <Tip label={name}>
      <button
        aria-label={name}
        onClick={() => onSection(n)}
        className="ml-auto shrink-0 rounded-sm border border-border-strong bg-elevated px-1 font-mono text-[10px] leading-4 text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
      >
        {sectionBadge(n)}
      </button>
    </Tip>
  );
}

function ModelCard({ model: m, ...links }: { model: Model } & Links) {
  return (
    <div className={cn("min-w-0 rounded-md border bg-panel text-[12px]", m.status === "new" ? "border-added/50" : "border-border")}>
      <div className="flex items-center gap-1.5 border-b border-border px-2 py-1.5">
        <Mark status={m.status} />
        <span className="min-w-0 truncate font-mono font-semibold">{m.name}</span>
        {m.file && (
          <span className="min-w-0 truncate text-[11px] text-subtle" title={m.file}>
            {m.file.slice(m.file.lastIndexOf("/") + 1)}
          </span>
        )}
        <Badge n={m.section} {...links} />
      </div>
      {m.note && <div className="border-b border-border px-2 py-1 text-[11.5px] text-muted-foreground">{m.note}</div>}
      {m.fields.length > 0 && (
        <ul className="py-1">
          {m.fields.map((f, i) => (
            <li key={i} className={cn("flex min-w-0 items-baseline gap-1.5 px-2 py-0.5", f.status === "new" && "bg-add-bg")}>
              <Mark status={f.status} />
              <span className="shrink-0 font-mono">{f.name}</span>
              {f.note && (
                <span className="min-w-0 truncate text-[11.5px] text-subtle" title={f.note}>
                  {f.note}
                </span>
              )}
              <Badge n={f.section} {...links} />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * A flow drawn by Mermaid (lazy, and themed by the app's CSS: .guide-flow). Steps with a section
 * become buttons to it once drawn; the steps in words are there for screen readers, and shown
 * instead when Mermaid can't draw it.
 */
function FlowDiagram({ flow, titles, onSection }: { flow: Flow } & Links) {
  const { dark } = useSettings();
  const code = useMemo(() => flowSource(flow), [flow]);
  const key = `${dark}\0${code}`;
  // Drawn before (a tab switch, a theme back): shown at once, not after a "Drawing" flash.
  const [drawn, setDrawn] = useState<{ key: string; svg?: string; error?: string } | null>(() => {
    const svg = drawnSvg(code, dark);
    return svg ? { key, svg } : null;
  });
  useEffect(() => {
    let live = true;
    draw(code, dark).then(
      (svg) => live && setDrawn({ key, svg }),
      (e) => live && setDrawn({ key, error: errorMessage(e) }),
    );
    return () => {
      live = false;
    };
  }, [code, dark, key]);
  const box = useRef<HTMLDivElement>(null);
  const svg = drawn?.svg;

  useLayoutEffect(() => {
    const el = box.current?.querySelector("svg");
    if (!el) return;
    // At its own size, scrolling sideways when wider: scaled to fit, a long flow's labels are unreadable.
    const width = el.viewBox.baseVal?.width;
    if (width) Object.assign(el.style, { maxWidth: "none", width: `${width}px` });
    // Mermaid names a step's node after its id (flowSource's s0, s1…): "flowchart-s3-<n>", after
    // the diagram's id and a dash in some 11.x releases.
    for (const g of el.querySelectorAll<SVGGElement>("g.node")) {
      const step = flow.steps[Number(/(?:^|-)flowchart-s(\d+)-\d+$/.exec(g.id)?.[1])];
      if (!step?.section) continue;
      g.dataset.section = String(step.section);
      g.setAttribute("tabindex", "0");
      g.setAttribute("role", "button");
      g.setAttribute("aria-label", `${step.label}, ${sectionName(step.section, titles)}`);
    }
  }, [svg, flow, titles]);

  const go = (e: SyntheticEvent) => {
    const n = Number((e.target as Element).closest<SVGElement>("[data-section]")?.dataset.section);
    if (!n) return;
    e.preventDefault();
    onSection(n);
  };
  const steps = describeFlow(flow);
  const failed = drawn?.key === key && drawn.error;
  return (
    <div>
      <h3 className="mb-1.5 text-[12px] font-medium text-muted-foreground">{flow.title || "Flow"}</h3>
      {failed ? (
        <div className="rounded-md border border-border p-3 text-[12px]">
          <div className="mb-1.5 text-destructive select-text">Could not draw this flow: {failed}</div>
          <ol className="list-decimal pl-5 text-muted-foreground select-text">
            {steps.map((s, i) => (
              <li key={i}>{s}</li>
            ))}
          </ol>
        </div>
      ) : (
        <>
          <ol className="sr-only">
            {steps.map((s, i) => (
              <li key={i}>{s}</li>
            ))}
          </ol>
          {svg ? (
            <div
              ref={box}
              role="group"
              aria-label={`${flow.title || "Flow"}: its steps link to their sections`}
              className="guide-flow overflow-x-auto rounded-md border border-border p-3"
              onClick={go}
              onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && go(e)}
              // Mermaid's output under securityLevel "strict", from labels flowSource escaped.
              dangerouslySetInnerHTML={{ __html: svg }}
            />
          ) : (
            <div className="rounded-md border border-border p-3 text-[12px] text-subtle">Drawing the flow…</div>
          )}
        </>
      )}
    </div>
  );
}
