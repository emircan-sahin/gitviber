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

/** Previews a media file, side by side when both the before and after versions exist. */
export function MediaView({ src, before, after }: { src: MediaSource; before: boolean; after: boolean }) {
  const both = before && after;
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
 * Renders SVG source as an image, before and after when both are given: stacked or side by side.
 * Both sides share one zoom, so they stay lined up while comparing.
 */
export function SvgView({ before, after, stacked, ...props }: { before: string | null; after: string | null; stacked: boolean } & SvgProps) {
  const both = before !== null && after !== null;
  return (
    <Sides stacked={stacked}>
      {before !== null && <SvgSide text={before} label={both ? "Before" : undefined} tone="removed" {...props} />}
      {after !== null && <SvgSide text={after} label={both ? "After" : undefined} tone="added" {...props} />}
    </Sides>
  );
}

function SvgSide({ text, label, tone, zoom, onZoom, backdrop }: { text: string; label?: string; tone: Tone } & SvgProps) {
  // Through <img> from a blob, never inline: scripts, handlers and external references in the SVG don't run.
  const { url, size } = useSvgUrl(text);
  const [natural, setNatural] = useState<[number, number] | null>(null);
  // Tied to the URL, so fixing the markup brings the image back.
  const [brokenUrl, setBrokenUrl] = useState<string | null>(null);
  const broken = url !== null && brokenUrl === url;
  const problem = useMemo(() => (broken ? svgProblem(text) : null), [broken, text]);
  const [stage, setStage] = useState<HTMLDivElement | null>(null);
  const [roomW, roomH] = useSize(stage);
  const fit = natural && roomW > 0 && roomH > 0 ? Math.max(0, Math.min((roomW - 2 * PAD) / natural[0], (roomH - 2 * PAD) / natural[1])) : 1;
  // Clamped here too: the file can change under a zoom chosen for a much smaller drawing.
  const scale = natural ? Math.min(zoom.scale ?? fit, zoomLimits(natural, fit)[1]) : fit;
  const [w, h] = natural ? [natural[0] * scale, natural[1] * scale] : [0, 0];
  const pannable = w > roomW || h > roomH;

  // Updates go through the updater form: several wheel or move events can land in one frame.
  const layout = useRef({ roomW, roomH, fit, natural });
  useLayoutEffect(() => {
    layout.current = { roomW, roomH, fit, natural };
  });
  /** Zooms by `factor`, keeping the point (x, y) of the stage where it is. */
  const zoomAt = useCallback(
    (factor: number, x: number, y: number) => {
      const { roomW, roomH, fit, natural } = layout.current;
      if (!natural) return;
      const [min, max] = zoomLimits(natural, fit);
      onZoom((z) => {
        const from = Math.min(z.scale ?? fit, max);
        const to = Math.min(max, Math.max(min, from * factor));
        return { scale: to, u: zoomAxis(roomW, natural[0] * from, natural[0] * to, z.u, x), v: zoomAxis(roomH, natural[1] * from, natural[1] * to, z.v, y) };
      });
    },
    [onZoom],
  );
  useEffect(() => {
    if (!stage) return;
    const wheel = (e: WheelEvent) => {
      if (!layout.current.natural) return;
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
    if (!natural) return;
    onZoom((z) => {
      const s = Math.min(z.scale ?? fit, zoomLimits(natural, fit)[1]);
      return { ...z, u: panAxis(roomW, natural[0] * s, z.u, dx), v: panAxis(roomH, natural[1] * s, z.v, dy) };
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
    if (!natural) return;
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

  return (
    <Panel label={label} tone={tone} details={[!broken && natural && `${natural[0]}×${natural[1]}`, !broken && natural && `${Math.round(scale * 100)}%`, url && formatBytes(size)]}>
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
            <img
              src={url}
              alt=""
              draggable={false}
              onLoad={(e) => setNatural(svgSize(text, [e.currentTarget.naturalWidth, e.currentTarget.naturalHeight]))}
              onError={() => setBrokenUrl(url)}
              // Hidden until its size is known, or it flashes at the webview's default size.
              style={
                natural
                  ? { left: Math.round(place(roomW, w, zoom.u)), top: Math.round(place(roomH, h, zoom.v)), width: Math.round(w), height: Math.round(h) }
                  : { visibility: "hidden" }
              }
              className={cn(BACKDROPS[backdrop], "pointer-events-none absolute max-w-none")}
            />
          )
        )}
      </div>
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
