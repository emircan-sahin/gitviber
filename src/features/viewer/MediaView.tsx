import { type Dispatch, type ReactNode, type SetStateAction, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, type DiffKind, errorMessage } from "@/lib/api";
import { matchesCommand } from "@/lib/commands/keybindings";
import { FIT, panAxis, place, svgSize, type Zoom, zoomAxis, zoomLimits } from "@/lib/ui/svg";
import { cn } from "@/lib/utils";
import { basename } from "@/lib/path";
import { Copy } from "lucide-react";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "@/components/ui/context-menu";
import { copyFiles } from "@/lib/app/clipboard";
import { failed } from "@/lib/app/toast";
import { IS_MAC } from "@/lib/platform";
import type { ImageCompare } from "@/lib/settings";

type MediaKind = "image" | "video" | "audio" | "pdf";

// SVG stays out: it is text, so it gets the code view and a real diff.
const TYPES: Record<string, [MediaKind, string]> = {
  png: ["image", "image/png"],
  jpg: ["image", "image/jpeg"],
  jpeg: ["image", "image/jpeg"],
  gif: ["image", "image/gif"],
  webp: ["image", "image/webp"],
  avif: ["image", "image/avif"],
  bmp: ["image", "image/bmp"],
  ico: ["image", "image/x-icon"],
  tif: ["image", "image/tiff"],
  tiff: ["image", "image/tiff"],
  heic: ["image", "image/heic"],
  mp4: ["video", "video/mp4"],
  m4v: ["video", "video/x-m4v"],
  mov: ["video", "video/quicktime"],
  webm: ["video", "video/webm"],
  ogv: ["video", "video/ogg"],
  mp3: ["audio", "audio/mpeg"],
  wav: ["audio", "audio/wav"],
  m4a: ["audio", "audio/mp4"],
  aac: ["audio", "audio/aac"],
  flac: ["audio", "audio/flac"],
  ogg: ["audio", "audio/ogg"],
  oga: ["audio", "audio/ogg"],
  opus: ["audio", "audio/ogg"],
  aif: ["audio", "audio/aiff"],
  aiff: ["audio", "audio/aiff"],
  pdf: ["pdf", "application/pdf"],
};

function typeOf(path: string) {
  const name = basename(path).toLowerCase();
  const dot = name.lastIndexOf(".");
  return dot > 0 ? TYPES[name.slice(dot + 1)] : undefined;
}

export function mediaKind(path: string): MediaKind | null {
  return typeOf(path)?.[0] ?? null;
}

export interface MediaSource {
  kind: DiffKind;
  path: string;
  oldPath: string | null;
  sha: string | null;
  base: string | null;
  /** Changes whenever the content may have changed (see pairArgs). */
  key: string;
}

/** Previews a media file, side by side when both the before and after versions exist, or an image's one over the other. */
export function MediaView({ src, before, after, compare }: { src: MediaSource; before: boolean; after: boolean; compare: ImageCompare }) {
  const both = before && after;
  if (both && compare !== "side" && isImageChange(src)) return <ImageOverlay src={src} mode={compare} />;
  return (
    <Sides stacked={false}>
      {before && <Side src={src} original label={both ? "Before" : undefined} tone="removed" />}
      {after && <Side src={src} original={false} label={both ? "After" : undefined} tone="added" />}
    </Sides>
  );
}

function Side({ src, original, label, tone }: { src: MediaSource; original: boolean; label?: string; tone: Tone }) {
  const { url, size, error } = useMediaUrl(src, original);
  const [dims, setDims] = useState<string | null>(null);
  const kind = mediaKind(original ? (src.oldPath ?? src.path) : src.path);

  return (
    <Panel label={label} tone={tone} details={[dims, size !== null && formatBytes(size)]}>
      {error ? (
        <div className="text-[12.5px] text-muted-foreground">{error}</div>
      ) : !url ? null : kind === "image" ? (
        <ImageMenu src={src} original={original}>
          <img
            src={url}
            onLoad={(e) => setDims(`${e.currentTarget.naturalWidth}×${e.currentTarget.naturalHeight}`)}
            className="checkerboard max-h-full max-w-full object-contain"
          />
        </ImageMenu>
      ) : kind === "video" ? (
        <video src={url} controls className="max-h-full max-w-full" />
      ) : kind === "audio" ? (
        <audio src={url} controls className="w-full max-w-md" />
      ) : (
        <iframe src={url} className="absolute inset-0 size-full border-0" />
      )}
    </Panel>
  );
}

/** Both versions are images, so they can be laid one over the other. */
export const isImageChange = (src: { path: string; oldPath: string | null }) => mediaKind(src.path) === "image" && mediaKind(src.oldPath ?? src.path) === "image";

type Size = [number, number];

/** The box two images of `sizes` share, both drawn from its top left corner. */
const frameOf = (sizes: (Size | null)[]): Size | null => (sizes.every(Boolean) ? [Math.max(...sizes.map((n) => n![0])), Math.max(...sizes.map((n) => n![1]))] : null);

const dims = (n: Size | null) => n && `${n[0]}×${n[1]}`;

/** The two versions of an image one over the other, shrunk together to fit like side by side (never enlarged). */
function ImageOverlay({ src, mode }: { src: MediaSource; mode: Exclude<ImageCompare, "side"> }) {
  const before = useMediaUrl(src, true);
  const after = useMediaUrl(src, false);
  const [sizes, setSizes] = useState<[Size | null, Size | null]>([null, null]);
  const [stage, setStage] = useState<HTMLDivElement | null>(null);
  const [roomW, roomH] = useSize(stage);
  const frame = frameOf(sizes);
  const scale = frame ? Math.max(0, Math.min(1, (roomW - 2 * PAD) / frame[0], (roomH - 2 * PAD) / frame[1])) : 1;
  const [left, top] = frame ? [Math.round((roomW - frame[0] * scale) / 2), Math.round((roomH - frame[1] * scale) / 2)] : [0, 0];
  const error = before.error ?? after.error;
  const layer = (url: string | null, i: 0 | 1) =>
    url && (
      <img
        src={url}
        alt=""
        draggable={false}
        onLoad={(e) => {
          const n: Size = [e.currentTarget.naturalWidth, e.currentTarget.naturalHeight];
          setSizes((s) => (i === 0 ? [n, s[1]] : [s[0], n]));
        }}
        // Hidden until both sizes are known, or the first one to load shows by itself.
        style={frame && sizes[i] ? { left, top, width: sizes[i]![0] * scale, height: sizes[i]![1] * scale } : { visibility: "hidden" }}
        className="pointer-events-none absolute max-w-none"
      />
    );
  return (
    <Overlay
      mode={mode}
      details={[
        [dims(sizes[0]), before.size !== null && formatBytes(before.size)],
        [dims(sizes[1]), after.size !== null && formatBytes(after.size)],
      ]}
      stageRef={setStage}
      under={
        error ? (
          <div className="flex h-full items-center justify-center text-[12.5px] text-muted-foreground">{error}</div>
        ) : (
          <>
            {frame && <div className="checkerboard absolute" style={{ left, top, width: frame[0] * scale, height: frame[1] * scale }} />}
            {layer(before.url, 0)}
          </>
        )
      }
      over={!error && layer(after.url, 1)}
    />
  );
}

/**
 * Before under after in one place. Swipe shows before left of a divider and after right of it;
 * onion skin fades after in over before. Only CSS on the two layers: nothing is decoded again.
 */
function Overlay({ mode, details, stageRef, under, over }: { mode: Exclude<ImageCompare, "side">; details: [(string | false | null)[], (string | false | null)[]]; stageRef?: (el: HTMLDivElement | null) => void; under: ReactNode; over: ReactNode }) {
  // Where the divider is, or how opaque after is: 0 is all before.
  const [at, setAt] = useState(0.5);
  const [b, a] = details.map((d) => d.filter(Boolean).join(" · "));
  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      <div className="flex h-7 shrink-0 items-center gap-2 border-b border-border px-3 text-[11.5px] text-subtle">
        <span className="font-medium text-removed">Before</span>
        <span className="font-mono">{b}</span>
        <div className="flex flex-1 justify-center">
          {mode === "onion" && <input type="range" min={0} max={100} value={Math.round(at * 100)} onChange={(e) => setAt(Number(e.target.value) / 100)} aria-label="Opacity of After" className="w-40 accent-primary" />}
        </div>
        <span className="font-mono">{a}</span>
        <span className="font-medium text-added">After</span>
      </div>
      <div ref={stageRef} className="relative min-h-0 flex-1 overflow-hidden">
        <div className="absolute inset-0">{under}</div>
        <div className="absolute inset-0" style={mode === "swipe" ? { clipPath: `inset(0 0 0 ${at * 100}%)` } : { opacity: at }}>
          {over}
        </div>
        {mode === "swipe" && <Divider at={at} onMove={setAt} />}
      </div>
    </div>
  );
}

/** Swipe's divider: dragged, or moved with the arrow keys. */
function Divider({ at, onMove }: { at: number; onMove: (at: number) => void }) {
  const drag = (e: React.PointerEvent<HTMLDivElement>) => {
    const box = e.currentTarget.parentElement!.getBoundingClientRect();
    onMove(Math.min(1, Math.max(0, (e.clientX - box.left) / box.width)));
  };
  return (
    <div
      role="slider"
      tabIndex={0}
      aria-label="Divider between Before and After"
      aria-valuenow={Math.round(at * 100)}
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId);
        drag(e);
      }}
      onPointerMove={(e) => e.currentTarget.hasPointerCapture(e.pointerId) && drag(e)}
      onKeyDown={(e) => {
        const step = e.key === "ArrowLeft" ? -0.05 : e.key === "ArrowRight" ? 0.05 : 0;
        if (!step) return;
        e.preventDefault();
        onMove(Math.min(1, Math.max(0, at + step)));
      }}
      className="group absolute inset-y-0 -ml-2 w-4 cursor-ew-resize outline-none"
      style={{ left: `${at * 100}%` }}
    >
      <div className="mx-auto h-full w-px bg-primary" />
      <div className="absolute top-1/2 left-1/2 size-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-primary bg-background group-focus-visible:ring-2 group-focus-visible:ring-ring" />
    </div>
  );
}

/** Right-click "Copy Image" on one side: the working tree's own file, or a stored version saved first. */
function ImageMenu({ src, original, children }: { src: MediaSource; original: boolean; children: ReactNode }) {
  if (!IS_MAC) return children;
  const copy = () =>
    api
      .mediaFile(src.kind, src.path, src.oldPath, src.sha, src.base, original)
      .then((path) => copyFiles([path]))
      .catch(failed("Could not copy"));
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={copy}>
          <Copy /> Copy Image
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

type Tone = "added" | "removed";

function Sides({ stacked, children }: { stacked: boolean; children: ReactNode }) {
  return <div className={cn("flex h-full min-h-0 overflow-hidden", stacked ? "flex-col divide-y divide-border" : "divide-x divide-border")}>{children}</div>;
}

/** One side of a preview: a header with its label and details, then the content, centered. */
function Panel({ label, tone, details, children }: { label?: string; tone: Tone; details: (string | false | null)[]; children: ReactNode }) {
  const detail = details.filter(Boolean).join(" · ");
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      {(label || detail) && (
        <div className="flex h-7 shrink-0 items-center gap-2 border-b border-border px-3 text-[11.5px] text-subtle">
          {label && <span className={cn("font-medium", tone === "added" ? "text-added" : "text-removed")}>{label}</span>}
          <span className="ml-auto font-mono">{detail}</span>
        </div>
      )}
      <div className="relative flex min-h-0 flex-1 items-center justify-center overflow-auto p-4">{children}</div>
    </div>
  );
}

export function isSvg(path: string) {
  return /\.svg$/i.test(path);
}

export type Backdrop = "theme" | "light" | "dark";

// The fixed backdrops rescue art drawn in a color that vanishes on the theme's.
const BACKDROPS: Record<Backdrop, string> = { theme: "checkerboard", light: "checkerboard-light", dark: "checkerboard-dark" };
/** Margin around an image fitted to its panel. */
const PAD = 16;

interface SvgProps {
  zoom: Zoom;
  onZoom: Dispatch<SetStateAction<Zoom>>;
  backdrop: Backdrop;
}

/**
 * Renders SVG source as an image, before and after when both are given: stacked, side by side, or
 * one over the other. Both sides share one zoom, so they stay lined up while comparing.
 */
export function SvgView({ before, after, stacked, compare, ...props }: { before: string | null; after: string | null; stacked: boolean; compare: ImageCompare } & SvgProps) {
  const both = before !== null && after !== null;
  const [sizes, setSizes] = useState<[Size | null, Size | null]>([null, null]);
  if (both && compare !== "side") {
    const frame = frameOf(sizes);
    return (
      <Overlay
        mode={compare}
        details={[[dims(sizes[0])], [dims(sizes[1])]]}
        under={<SvgSide text={before} layer={{ frame, onSize: (n) => setSizes((s) => [n, s[1]]), backdrop: true }} {...props} />}
        over={<SvgSide text={after} layer={{ frame, onSize: (n) => setSizes((s) => [s[0], n]), backdrop: false }} {...props} />}
      />
    );
  }
  return (
    <Sides stacked={stacked}>
      {before !== null && <SvgSide text={before} label={both ? "Before" : undefined} tone="removed" {...props} />}
      {after !== null && <SvgSide text={after} label={both ? "After" : undefined} tone="added" {...props} />}
    </Sides>
  );
}

/** One of two SVGs laid over each other: both are placed in `frame` (null until both sizes are known), backdrop only under the first. */
interface SvgLayer {
  frame: Size | null;
  onSize: (n: Size) => void;
  backdrop: boolean;
}

function SvgSide({ text, label, tone = "added", zoom, onZoom, backdrop, layer }: { text: string; label?: string; tone?: Tone; layer?: SvgLayer } & SvgProps) {
  // Through <img> from a blob, never inline: scripts, handlers and external references in the SVG don't run.
  const { url, size } = useSvgUrl(text);
  const [natural, setNatural] = useState<[number, number] | null>(null);
  // Tied to the URL, so fixing the markup brings the image back.
  const [brokenUrl, setBrokenUrl] = useState<string | null>(null);
  const broken = url !== null && brokenUrl === url;
  const problem = useMemo(() => (broken ? svgProblem(text) : null), [broken, text]);
  const [stage, setStage] = useState<HTMLDivElement | null>(null);
  const [roomW, roomH] = useSize(stage);
  // What's fitted, zoomed and moved: the drawing, or the frame it shares with the other layer.
  const box = layer ? (natural && layer.frame) : natural;
  const fit = box && roomW > 0 && roomH > 0 ? Math.max(0, Math.min((roomW - 2 * PAD) / box[0], (roomH - 2 * PAD) / box[1])) : 1;
  // Clamped here too: the file can change under a zoom chosen for a much smaller drawing.
  const scale = box ? Math.min(zoom.scale ?? fit, zoomLimits(box, fit)[1]) : fit;
  const [w, h] = box ? [box[0] * scale, box[1] * scale] : [0, 0];
  const pannable = w > roomW || h > roomH;

  // Updates go through the updater form: several wheel or move events can land in one frame.
  const layout = useRef({ roomW, roomH, fit, box });
  useLayoutEffect(() => {
    layout.current = { roomW, roomH, fit, box };
  });
  /** Zooms by `factor`, keeping the point (x, y) of the stage where it is. */
  const zoomAt = useCallback(
    (factor: number, x: number, y: number) => {
      const { roomW, roomH, fit, box } = layout.current;
      if (!box) return;
      const [min, max] = zoomLimits(box, fit);
      onZoom((z) => {
        const from = Math.min(z.scale ?? fit, max);
        const to = Math.min(max, Math.max(min, from * factor));
        return { scale: to, u: zoomAxis(roomW, box[0] * from, box[0] * to, z.u, x), v: zoomAxis(roomH, box[1] * from, box[1] * to, z.v, y) };
      });
    },
    [onZoom],
  );
  useEffect(() => {
    if (!stage) return;
    const wheel = (e: WheelEvent) => {
      if (!layout.current.box) return;
      e.preventDefault();
      // Pinches arrive as ctrl+wheel with small deltas; line-mode wheels scroll ~3 lines a notch.
      const delta = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY;
      const r = stage.getBoundingClientRect();
      zoomAt(Math.exp(-delta * (e.ctrlKey ? 0.01 : 0.002)), e.clientX - r.left, e.clientY - r.top);
    };
    stage.addEventListener("wheel", wheel, { passive: false });
    return () => stage.removeEventListener("wheel", wheel);
  }, [stage, zoomAt]);

  const panBy = (dx: number, dy: number) => {
    if (!box) return;
    onZoom((z) => {
      const s = Math.min(z.scale ?? fit, zoomLimits(box, fit)[1]);
      return { ...z, u: panAxis(roomW, box[0] * s, z.u, dx), v: panAxis(roomH, box[1] * s, z.v, dy) };
    });
  };
  const drag = useRef<{ x: number; y: number } | null>(null);
  const pan = (e: React.PointerEvent) => {
    const from = drag.current;
    if (!from) return;
    drag.current = { x: e.clientX, y: e.clientY };
    panBy(e.clientX - from.x, e.clientY - from.y);
  };
  // The keyboard's wheel and drag: + / − / 0 zoom around the middle, arrows move the view.
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!box) return;
    const step = 48;
    if (matchesCommand("media.zoomIn", e.nativeEvent)) zoomAt(1.25, roomW / 2, roomH / 2);
    else if (matchesCommand("media.zoomOut", e.nativeEvent)) zoomAt(0.8, roomW / 2, roomH / 2);
    else if (matchesCommand("media.fit", e.nativeEvent)) onZoom(FIT);
    else if (e.metaKey || e.ctrlKey || e.altKey) return;
    else if (e.key === "ArrowLeft") panBy(step, 0);
    else if (e.key === "ArrowRight") panBy(-step, 0);
    else if (e.key === "ArrowUp") panBy(0, step);
    else if (e.key === "ArrowDown") panBy(0, -step);
    else return;
    e.preventDefault();
  };

  // The box's corner; a layer draws its drawing from there at its own size.
  const [x, y] = [Math.round(place(roomW, w, zoom.u)), Math.round(place(roomH, h, zoom.v))];
  const [drawnW, drawnH] = layer && natural ? [natural[0] * scale, natural[1] * scale] : [w, h];
  const view = (
    <div
      ref={setStage}
      // Where focusPanel("code") lands (see panels.ts), so the keys below work from F6 too.
      data-code-scroll
      tabIndex={0}
      aria-label={`${label ? `${label}: ` : ""}SVG preview. + and − zoom, 0 fits, arrows move it`}
      onKeyDown={onKeyDown}
      onPointerDown={(e) => {
        if (e.button !== 0 || !pannable) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        drag.current = { x: e.clientX, y: e.clientY };
      }}
      onPointerMove={pan}
      onPointerUp={() => (drag.current = null)}
      onPointerCancel={() => (drag.current = null)}
      onDoubleClick={() => onZoom(FIT)}
      className={cn("absolute inset-0 overflow-hidden outline-none", pannable && "cursor-grab active:cursor-grabbing")}
    >
      {broken ? (
        <div className="flex h-full flex-col items-center justify-center p-4 text-center">
          <div className="text-[12.5px] text-muted-foreground">This SVG can't be rendered</div>
          <div className="mt-1 max-w-xl text-[11.5px] text-subtle select-text">{problem ?? "Its markup is likely invalid. Switch to Code to see it."}</div>
        </div>
      ) : (
        url && (
          <>
            {layer?.backdrop && box && <div className={cn(BACKDROPS[backdrop], "pointer-events-none absolute")} style={{ left: x, top: y, width: Math.round(w), height: Math.round(h) }} />}
            <img
              src={url}
              alt=""
              draggable={false}
              onLoad={(e) => {
                const n = svgSize(text, [e.currentTarget.naturalWidth, e.currentTarget.naturalHeight]);
                setNatural(n);
                layer?.onSize(n);
              }}
              onError={() => setBrokenUrl(url)}
              // Hidden until its size is known, or it flashes at the webview's default size.
              style={box ? { left: x, top: y, width: Math.round(drawnW), height: Math.round(drawnH) } : { visibility: "hidden" }}
              className={cn(!layer && BACKDROPS[backdrop], "pointer-events-none absolute max-w-none")}
            />
          </>
        )
      )}
    </div>
  );
  if (layer) return view;
  return (
    <Panel label={label} tone={tone} details={[!broken && natural && `${natural[0]}×${natural[1]}`, !broken && natural && `${Math.round(scale * 100)}%`, url && formatBytes(size)]}>
      {view}
    </Panel>
  );
}

/** Why an SVG that failed to load can't render, from the browser's XML parser (which runs nothing in it). */
export function svgProblem(text: string): string | null {
  const doc = new DOMParser().parseFromString(text, "image/svg+xml");
  const error = doc.querySelector("parsererror");
  if (error) return (error.querySelector("div") ?? error).textContent?.trim() || null;
  const root = doc.documentElement;
  if (root.localName !== "svg") return `The root element is <${root.localName}>, not <svg>.`;
  if (root.namespaceURI !== "http://www.w3.org/2000/svg") return `The <svg> tag has no xmlns="http://www.w3.org/2000/svg", so it only renders inlined in HTML.`;
  return null;
}

function useSvgUrl(text: string) {
  const [state, setState] = useState<{ url: string | null; size: number }>({ url: null, size: 0 });
  useEffect(() => {
    const blob = new Blob([text], { type: "image/svg+xml" });
    const url = URL.createObjectURL(blob);
    setState({ url, size: blob.size });
    return () => URL.revokeObjectURL(url);
  }, [text]);
  return state;
}

function useSize(el: HTMLElement | null): [number, number] {
  const [size, setSize] = useState<[number, number]>([0, 0]);
  useEffect(() => {
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => setSize([entry.contentRect.width, entry.contentRect.height]));
    observer.observe(el);
    return () => observer.disconnect();
  }, [el]);
  return size;
}

export function useMediaUrl(src: MediaSource, original: boolean) {
  const [state, setState] = useState<{ url: string | null; size: number | null; error: string | null }>({ url: null, size: null, error: null });
  const current = useRef<string | null>(null);
  const { kind, path, oldPath, sha, base, key } = src;

  useEffect(() => {
    let live = true;
    const file = original ? (oldPath ?? path) : path;
    // SVG is not in TYPES (it is text), but markdown can still embed it as an image.
    const mime = typeOf(file)?.[1] ?? (isSvg(file) ? "image/svg+xml" : undefined);
    // The old URL stays until its replacement is ready, so a working-tree refresh doesn't blank the view.
    const show = (url: string | null, size: number | null, error: string | null) => {
      if (current.current) URL.revokeObjectURL(current.current);
      current.current = url;
      setState({ url, size, error });
    };
    api
      .media(kind, path, oldPath, sha, base, original)
      .then((bytes) => live && show(URL.createObjectURL(new Blob([bytes], { type: mime })), bytes.byteLength, null))
      .catch((e) => live && show(null, null, errorMessage(e)));
    return () => {
      live = false;
    };
  }, [key, kind, path, oldPath, sha, base, original]);
  useEffect(() => () => void (current.current && URL.revokeObjectURL(current.current)), []);
  return state;
}

function formatBytes(n: number) {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}
