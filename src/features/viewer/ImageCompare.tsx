// A changed image's two versions one over the other, as GitHub offers beside 2-up: Swipe (a
// divider between them) and Onion skin (after faded in over before). Only CSS on the two layers
// already shown side by side: nothing is decoded again.
import { type ReactElement, type ReactNode, useEffect, useState } from "react";
import type { ImageCompare } from "@/lib/settings";
import { formatBytes, ImageMenu, type MediaSource, PAD, SvgSide, type SvgProps, useMediaUrl, useSize } from "./MediaView";

export type Overlaid = Exclude<ImageCompare, "side">;

type Size = [number, number];

/** The box two images of `sizes` share, both drawn from its top left corner. */
const frameOf = (sizes: (Size | null)[]): Size | null => (sizes.every(Boolean) ? [Math.max(...sizes.map((n) => n![0])), Math.max(...sizes.map((n) => n![1]))] : null);

const dims = (n: Size | null) => n && `${n[0]}×${n[1]}`;

/** Both versions of an image, shrunk together to fit like side by side (never enlarged). Keyed by the source, so sizes start over. */
export function ImageOverlay({ src, mode }: { src: MediaSource; mode: Overlaid }) {
  const before = useMediaUrl(src, true);
  const after = useMediaUrl(src, false);
  const [sizes, setSizes] = useState<[Size | null, Size | null]>([null, null]);
  const [broken, setBroken] = useState(false);
  const [stage, setStage] = useState<HTMLDivElement | null>(null);
  const [roomW, roomH] = useSize(stage);
  const frame = frameOf(sizes);
  const scale = frame ? Math.max(0, Math.min(1, (roomW - 2 * PAD) / frame[0], (roomH - 2 * PAD) / frame[1])) : 1;
  const box = frame && { left: Math.round((roomW - frame[0] * scale) / 2), top: Math.round((roomH - frame[1] * scale) / 2), width: frame[0] * scale, height: frame[1] * scale };
  const error = before.error ?? after.error ?? (broken ? "One of the versions can't be shown here. 2-up shows the other." : null);
  // `backdrop`: its own, frame-sized; onion skin fades after alone over before's.
  const layer = (url: string | null, i: 0 | 1, backdrop: boolean) =>
    url && (
      <>
        {backdrop && box && <div className="checkerboard absolute" style={box} />}
        <img
          src={url}
          alt=""
          draggable={false}
          onLoad={(e) => {
            const n: Size = [e.currentTarget.naturalWidth, e.currentTarget.naturalHeight];
            setSizes((s) => (i === 0 ? [n, s[1]] : [s[0], n]));
          }}
          // Both wait for the other's size, so one that can't decode would leave the stage blank.
          onError={() => setBroken(true)}
          style={box && sizes[i] ? { left: box.left, top: box.top, width: sizes[i]![0] * scale, height: sizes[i]![1] * scale } : { visibility: "hidden" }}
          className="pointer-events-none absolute max-w-none"
        />
      </>
    );
  return (
    <Overlay
      mode={mode}
      details={[
        [dims(sizes[0]), before.size !== null && formatBytes(before.size)],
        [dims(sizes[1]), after.size !== null && formatBytes(after.size)],
      ]}
      stageRef={setStage}
      wrapStage={error ? undefined : (stage) => <ImageMenu src={src} original="both">{stage}</ImageMenu>}
      under={error ? <div className="flex h-full items-center justify-center p-4 text-center text-[12.5px] text-muted-foreground">{error}</div> : layer(before.url, 0, true)}
      over={!error && layer(after.url, 1, mode === "swipe")}
    />
  );
}

/** Both versions of an SVG in one frame, with the zoom and pan of the preview. */
export function SvgOverlay({ before, after, mode, ...props }: { before: string; after: string; mode: Overlaid } & SvgProps) {
  const [sizes, setSizes] = useState<[Size | null, Size | null]>([null, null]);
  // A new version brings its own size: none until it loads.
  useEffect(() => setSizes([null, null]), [before, after]);
  const frame = frameOf(sizes);
  return (
    <Overlay
      mode={mode}
      details={[[dims(sizes[0])], [dims(sizes[1])]]}
      under={<SvgSide text={before} layer={{ frame, onSize: (n) => setSizes((s) => [n, s[1]]), backdrop: true }} {...props} />}
      over={<SvgSide text={after} layer={{ frame, onSize: (n) => setSizes((s) => [s[0], n]), backdrop: mode === "swipe" }} {...props} />}
    />
  );
}

/**
 * Before under after in one place: swipe shows before left of a divider and after right of it; onion skin fades after in over before.
 * `wrapStage`: puts the stage in a context menu.
 */
function Overlay({
  mode,
  details,
  stageRef,
  wrapStage = (stage) => stage,
  under,
  over,
}: {
  mode: Overlaid;
  details: [(string | false | null)[], (string | false | null)[]];
  stageRef?: (el: HTMLDivElement | null) => void;
  wrapStage?: (stage: ReactElement) => ReactNode;
  under: ReactNode;
  over: ReactNode;
}) {
  // Kept apart: a divider near the edge isn't a faint onion skin.
  const [divider, setDivider] = useState(0.5);
  const [opacity, setOpacity] = useState(0.5);
  const [b, a] = details.map((d) => d.filter(Boolean).join(" · "));
  const swipe = mode === "swipe";
  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      <div className="flex h-7 shrink-0 items-center gap-2 border-b border-border px-3 text-[11.5px] text-subtle">
        <span className="font-medium text-removed">Before</span>
        <span className="font-mono">{b}</span>
        <div className="flex flex-1 justify-center">
          {!swipe && <input type="range" min={0} max={100} value={Math.round(opacity * 100)} onChange={(e) => setOpacity(Number(e.target.value) / 100)} aria-label="Opacity of After" className="w-40 accent-primary" />}
        </div>
        <span className="font-mono">{a}</span>
        <span className="font-medium text-added">After</span>
      </div>
      {wrapStage(
        <div ref={stageRef} className="relative min-h-0 flex-1 overflow-hidden">
          <div className="absolute inset-0" style={swipe ? { clipPath: `inset(0 ${(1 - divider) * 100}% 0 0)` } : undefined}>
            {under}
          </div>
          <div className="absolute inset-0" style={swipe ? { clipPath: `inset(0 0 0 ${divider * 100}%)` } : { opacity }}>
            {over}
          </div>
          {swipe && <Divider at={divider} onMove={setDivider} />}
        </div>,
      )}
    </div>
  );
}

/** Swipe's divider: dragged, or moved with the arrow keys. */
function Divider({ at, onMove }: { at: number; onMove: (at: number) => void }) {
  const clamp = (v: number) => Math.min(1, Math.max(0, v));
  const drag = (e: React.PointerEvent<HTMLDivElement>) => {
    const box = e.currentTarget.parentElement!.getBoundingClientRect();
    onMove(clamp((e.clientX - box.left) / box.width));
  };
  return (
    <div
      role="slider"
      tabIndex={0}
      aria-label="Divider between Before and After"
      aria-valuemin={0}
      aria-valuemax={100}
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
        onMove(clamp(at + step));
      }}
      className="group absolute inset-y-0 -ml-2 w-4 cursor-ew-resize outline-none"
      style={{ left: `${at * 100}%` }}
    >
      <div className="mx-auto h-full w-px bg-primary" />
      <div className="absolute top-1/2 left-1/2 size-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-primary bg-background group-focus-visible:ring-2 group-focus-visible:ring-ring" />
    </div>
  );
}
