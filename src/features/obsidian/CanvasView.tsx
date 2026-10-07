import { Globe, Maximize, Minus, Plus } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Tip } from "@/components/ui/tooltip";
import { errorMessage } from "@/lib/api";
import { matchesCommand } from "@/lib/commands/keybindings";
import { type Canvas, canvasColor, type CanvasEdge, type CanvasNode, edgePath, fitBox, parseCanvas } from "@/lib/obsidian/canvas";
import type { Selection } from "@/lib/repo/selection";
import { useSettings } from "@/lib/settings";
import { releaseIfButtonLost } from "@/lib/ui/held";
import { followLink } from "@/features/viewer/MarkdownView";
import { useSize } from "@/features/viewer/MediaView";
import { NoteFile, NoteScope, NoteText } from "./VaultMarkdown";

/** Where the canvas is looked at: its point (0, 0) on screen at (x, y), drawn `scale` times its size. */
interface View {
  x: number;
  y: number;
  scale: number;
}

const PAD = 40;
const [MIN, MAX, FIT_MIN] = [0.05, 4, 0.1];

/**
 * A JSON Canvas (Obsidian's .canvas): its cards, groups and arrows, read-only. Text cards render
 * as notes do, file cards show the file as an embed would, link cards open in the browser. Drag
 * or scroll to move around, pinch or ⌘-scroll to zoom, double-click the background to fit.
 */
export function CanvasView({ vault, path, text, onOpen }: { vault: string; path: string; text: string; onOpen: (s: Selection, pin?: boolean) => void }) {
  const parsed = useMemo((): { canvas: Canvas } | { error: string } => {
    try {
      return { canvas: parseCanvas(text) };
    } catch (e) {
      return { error: errorMessage(e) };
    }
  }, [text]);
  if ("error" in parsed)
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center">
        <div className="text-[12.5px] text-muted-foreground">This canvas can't be read</div>
        <div className="max-w-xl text-[11.5px] text-subtle select-text">{parsed.error}. Switch to Code to see it.</div>
      </div>
    );
  return (
    <NoteScope vault={vault} path={path} onOpen={onOpen}>
      <Board canvas={parsed.canvas} />
    </NoteScope>
  );
}

function Board({ canvas }: { canvas: Canvas }) {
  const { dark } = useSettings();
  const [stage, setStage] = useState<HTMLDivElement | null>(null);
  const [w, h] = useSize(stage);
  const box = useMemo(() => fitBox(canvas.nodes), [canvas]);
  const fit = useMemo((): View => {
    if (!w || !h || !box.width) return { x: w / 2, y: h / 2, scale: 1 };
    // Fitting never goes below 10%: two cards far apart would be dots; the rest is a pan away.
    const scale = Math.min(1, Math.max(FIT_MIN, Math.min((w - 2 * PAD) / box.width, (h - 2 * PAD) / box.height)));
    return { scale, x: (w - box.width * scale) / 2 - box.x * scale, y: (h - box.height * scale) / 2 - box.y * scale };
  }, [w, h, box]);
  // null: fitted, following the pane's size until the user moves.
  const [moved, setMoved] = useState<View | null>(null);
  const view = moved ?? fit;
  const current = useRef(view);
  useLayoutEffect(() => {
    current.current = view;
  });

  const zoomAt = useCallback((factor: number, px: number, py: number) => {
    const v = current.current;
    const scale = Math.min(MAX, Math.max(MIN, v.scale * factor));
    // The canvas point under (px, py) stays there.
    setMoved({ scale, x: px - ((px - v.x) * scale) / v.scale, y: py - ((py - v.y) * scale) / v.scale });
  }, []);
  const panBy = useCallback((dx: number, dy: number) => {
    const v = current.current;
    setMoved({ ...v, x: v.x + dx, y: v.y + dy });
  }, []);

  useEffect(() => {
    if (!stage) return;
    const wheel = (e: WheelEvent) => {
      // A card's own scrolling text scrolls while it can.
      const scroller = (e.target as HTMLElement).closest<HTMLElement>("[data-canvas-scroll]");
      if (scroller && !e.ctrlKey && !e.metaKey && Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
        const room = e.deltaY < 0 ? scroller.scrollTop > 0 : scroller.scrollTop + scroller.clientHeight < scroller.scrollHeight - 1;
        if (room) return;
      }
      e.preventDefault();
      const unit = e.deltaMode === 1 ? 16 : 1;
      const r = stage.getBoundingClientRect();
      // Pinches arrive as ctrl+wheel.
      if (e.ctrlKey || e.metaKey) zoomAt(Math.exp(-e.deltaY * unit * (e.ctrlKey ? 0.01 : 0.002)), e.clientX - r.left, e.clientY - r.top);
      else panBy(-e.deltaX * unit, -e.deltaY * unit);
    };
    stage.addEventListener("wheel", wheel, { passive: false });
    return () => stage.removeEventListener("wheel", wheel);
  }, [stage, zoomAt, panBy]);

  const drag = useRef<{ x: number; y: number } | null>(null);
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.target !== e.currentTarget) return;
    const step = 64;
    if (matchesCommand("media.zoomIn", e.nativeEvent)) zoomAt(1.25, w / 2, h / 2);
    else if (matchesCommand("media.zoomOut", e.nativeEvent)) zoomAt(0.8, w / 2, h / 2);
    else if (matchesCommand("media.fit", e.nativeEvent)) setMoved(null);
    else if (e.metaKey || e.ctrlKey || e.altKey) return;
    else if (e.key === "ArrowLeft") panBy(step, 0);
    else if (e.key === "ArrowRight") panBy(-step, 0);
    else if (e.key === "ArrowUp") panBy(0, step);
    else if (e.key === "ArrowDown") panBy(0, -step);
    else return;
    e.preventDefault();
  };

  const byId = useMemo(() => new Map(canvas.nodes.map((n) => [n.id, n])), [canvas]);
  const groups = canvas.nodes.filter((n) => n.type === "group");
  const cards = canvas.nodes.filter((n) => n.type !== "group");

  return (
    <div
      ref={setStage}
      data-code-scroll
      tabIndex={0}
      aria-label="Canvas. Drag or scroll to move, + and − zoom, 0 fits"
      onKeyDown={onKeyDown}
      onPointerDown={(e) => {
        // Only the background drags; cards keep their clicks and text selection.
        if (e.button !== 0 || (e.target as HTMLElement).closest("[data-canvas-node]")) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        drag.current = { x: e.clientX, y: e.clientY };
      }}
      onPointerMove={(e) => {
        const from = drag.current;
        if (!from || releaseIfButtonLost(e.nativeEvent)) return void (drag.current = null);
        drag.current = { x: e.clientX, y: e.clientY };
        panBy(e.clientX - from.x, e.clientY - from.y);
      }}
      onPointerUp={() => (drag.current = null)}
      onPointerCancel={() => (drag.current = null)}
      onDoubleClick={(e) => !(e.target as HTMLElement).closest("[data-canvas-node]") && setMoved(null)}
      className="canvas-board absolute inset-0 cursor-grab overflow-hidden outline-none active:cursor-grabbing"
      style={{ backgroundPosition: `${view.x}px ${view.y}px`, backgroundSize: `${24 * view.scale}px ${24 * view.scale}px` }}
    >
      {!canvas.nodes.length && <div className="flex h-full items-center justify-center text-[12.5px] text-muted-foreground">This canvas is empty</div>}
      <div className="absolute top-0 left-0" style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})`, transformOrigin: "0 0" }}>
        {groups.map((n) => (
          <Group key={n.id} node={n} dark={dark} />
        ))}
        <svg className="pointer-events-none absolute top-0 left-0 overflow-visible" width="1" height="1">
          {canvas.edges.map((e) => (
            <Edge key={e.id} edge={e} from={byId.get(e.fromNode)!} to={byId.get(e.toNode)!} dark={dark} />
          ))}
        </svg>
        {canvas.edges.map((e) => e.label && <EdgeLabel key={e.id} edge={e} from={byId.get(e.fromNode)!} to={byId.get(e.toNode)!} />)}
        {cards.map((n) => (
          <Card key={n.id} node={n} dark={dark} />
        ))}
      </div>
      <div className="absolute right-3 bottom-3 flex items-center gap-0.5 rounded-md border border-border bg-elevated p-0.5 shadow-sm">
        <ZoomButton label="Zoom out" onClick={() => zoomAt(0.8, w / 2, h / 2)}>
          <Minus />
        </ZoomButton>
        <span className="w-10 text-center font-mono text-[11px] text-subtle">{Math.round(view.scale * 100)}%</span>
        <ZoomButton label="Zoom in" onClick={() => zoomAt(1.25, w / 2, h / 2)}>
          <Plus />
        </ZoomButton>
        <ZoomButton label="Fit" onClick={() => setMoved(null)}>
          <Maximize />
        </ZoomButton>
      </div>
    </div>
  );
}

const place = (n: CanvasNode) => ({ left: n.x, top: n.y, width: n.width, height: n.height });

function Group({ node, dark }: { node: CanvasNode; dark: boolean }) {
  const color = canvasColor(node.color, dark);
  return (
    <div className="absolute rounded-xl border-2" style={{ ...place(node), borderColor: color ?? "var(--border-strong)", background: canvasColor(node.color, dark, 0.06) ?? "color-mix(in srgb, var(--foreground) 3%, transparent)" }}>
      {node.label && (
        <div className="absolute bottom-full left-0 mb-1.5 max-w-full truncate rounded-md px-2 py-0.5 text-[15px] font-medium" style={{ color: color ?? "var(--muted-foreground)" }}>
          {node.label}
        </div>
      )}
    </div>
  );
}

function Card({ node, dark }: { node: CanvasNode; dark: boolean }) {
  const color = canvasColor(node.color, dark);
  return (
    <div
      data-canvas-node
      className="absolute flex flex-col overflow-hidden rounded-xl border-2 bg-solid-panel shadow-sm"
      style={{ ...place(node), borderColor: color ?? "var(--border-strong)", backgroundImage: color ? `linear-gradient(${canvasColor(node.color, dark, 0.08)}, ${canvasColor(node.color, dark, 0.08)})` : undefined }}
    >
      <div data-canvas-scroll className="markdown min-h-0 flex-1 overflow-auto px-4 py-3 select-text">
        {node.type === "text" ? (
          <NoteText text={node.text ?? ""} />
        ) : node.type === "file" && node.file ? (
          <NoteFile file={node.file} subpath={node.subpath} />
        ) : node.type === "link" && node.url ? (
          <a
            href={node.url}
            onClick={(e) => {
              e.preventDefault();
              followLink(node.url!, () => {});
            }}
            className="flex items-center gap-2 break-all"
          >
            <Globe className="size-4 shrink-0" /> {node.url}
          </a>
        ) : null}
      </div>
    </div>
  );
}

function Edge({ edge, from, to, dark }: { edge: CanvasEdge; from: CanvasNode; to: CanvasNode; dark: boolean }) {
  const { d, start, end } = edgePath(from, to, edge.fromSide, edge.toSide);
  const color = canvasColor(edge.color, dark) ?? "var(--subtle)";
  return (
    <g style={{ color }}>
      <path d={d} fill="none" stroke="currentColor" strokeWidth={2.5} strokeLinecap="round" />
      {edge.toEnd === "arrow" && <Arrow at={end.at} angle={end.angle} />}
      {edge.fromEnd === "arrow" && <Arrow at={start.at} angle={start.angle} />}
    </g>
  );
}

/** An arrowhead with its tip at `at`, pointing along `angle`. */
function Arrow({ at: [x, y], angle }: { at: [number, number]; angle: number }) {
  const [len, half] = [14, 7];
  const [bx, by] = [x - Math.cos(angle) * len, y - Math.sin(angle) * len];
  const [px, py] = [-Math.sin(angle) * half, Math.cos(angle) * half];
  return <polygon points={`${x},${y} ${bx + px},${by + py} ${bx - px},${by - py}`} fill="currentColor" />;
}

function EdgeLabel({ edge, from, to }: { edge: CanvasEdge; from: CanvasNode; to: CanvasNode }) {
  const { mid } = edgePath(from, to, edge.fromSide, edge.toSide);
  return (
    <div className="absolute max-w-60 -translate-x-1/2 -translate-y-1/2 rounded-md border border-border bg-solid-background px-2 py-0.5 text-center text-[13px] text-muted-foreground" style={{ left: mid[0], top: mid[1] }}>
      {edge.label}
    </div>
  );
}

function ZoomButton({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <Tip label={label}>
      <button aria-label={label} onClick={onClick} className="flex size-6 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground focus-visible:bg-hover focus-visible:text-foreground [&_svg]:size-3.5">
        {children}
      </button>
    </Tip>
  );
}
