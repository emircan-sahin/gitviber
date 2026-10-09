import type { ReactNode, RefObject } from "react";
import type { Device } from "@/lib/browser/devices";
import type { Fitted } from "@/lib/browser/fit";

/** A phone's side buttons, as a share of its height from the top and a length in device CSS px: volume up and down on the left, power on the right. */
const SIDE_BUTTONS = [
  { side: "left", at: 0.2, length: 50 },
  { side: "left", at: 0.3, length: 50 },
  { side: "right", at: 0.25, length: 70 },
] as const;
/** How far a side button stands out of the body, in device CSS px. */
const BUTTON_DEPTH = 3;

interface Props {
  device: Device;
  rotated: boolean;
  fitted: Fitted;
  /** The page's element: the native view is laid exactly over it. */
  page: RefObject<HTMLDivElement | null>;
  /** Responsive: an edge's drag (right `dx`, bottom `dy`). */
  resize?: (dx: 0 | 1, dy: 0 | 1) => (e: React.PointerEvent<HTMLElement>) => void;
  /** What shows where the page goes while its view doesn't (a picture). */
  children?: ReactNode;
}

/**
 * A device's body, drawn from devices.json (no maker's artwork), with its screen: the status bar
 * and home indicator drawn here, the page between them. The page's native view covers only the
 * page, as the app page can't draw over it; the rest sits around it.
 */
export function DeviceFrame({ device, rotated, fitted, page, resize, children }: Props) {
  const { frame, screen, scale, bars } = fitted;
  const phone = device.platform === "ios" || device.platform === "android";
  const depth = BUTTON_DEPTH * scale;
  const buttons = phone
    ? SIDE_BUTTONS.map(({ side, at, length }) => {
        const along = length * scale;
        // Turned left, the left side is the bottom and the right side the top.
        if (rotated) return { x: at * frame.w, y: side === "left" ? frame.h : -depth, w: along, h: depth };
        return { x: side === "left" ? -depth : frame.w, y: at * frame.h, w: depth, h: along };
      })
    : [];
  const inScreen = (b: { x: number; y: number; w: number; h: number }) => ({ left: b.x - screen.x, top: b.y - screen.y, width: b.w, height: b.h });

  return (
    <>
      <svg aria-hidden className="absolute overflow-visible" style={{ left: frame.x, top: frame.y, width: frame.w, height: frame.h }}>
        {buttons.map((b, i) => (
          <rect key={i} {...b} rx={depth / 2} className="fill-border-strong" />
        ))}
        {device.bezel > 0 && <rect x={0.5} y={0.5} width={frame.w - 1} height={frame.h - 1} rx={(device.radius + device.bezel) * scale} className="fill-elevated stroke-border-strong" />}
      </svg>
      <div className="absolute overflow-hidden bg-white select-none" style={{ left: screen.x, top: screen.y, width: screen.w, height: screen.h, borderRadius: fitted.radius }}>
        {bars.top > 0 && <StatusBar fitted={fitted} />}
        {bars.left > 0 && <SideBar fitted={fitted} />}
        {bars.bottom > 0 && (
          <div className="absolute inset-x-0 bottom-0 flex items-center justify-center" style={{ height: bars.bottom }}>
            <div className="rounded-full bg-black" style={{ width: screen.w * 0.35, height: Math.max(1, 5 * scale) }} />
          </div>
        )}
        <div ref={page} tabIndex={-1} className="absolute overflow-hidden bg-background outline-none" style={inScreen(fitted.page)}>
          {children}
        </div>
      </div>
      {resize && <Handles fitted={fitted} resize={resize} />}
    </>
  );
}

/** The camera's island, notch or hole, from the screen's top left. */
function Camera({ fitted }: { fitted: Fitted }) {
  const c = fitted.cutout;
  return c && <div className="absolute bg-black" style={{ left: c.x, top: c.y, width: c.w, height: c.h, borderRadius: c.r }} />;
}

/** Upright: the time on the left, signal, Wi-Fi and battery on the right, the camera between. */
function StatusBar({ fitted }: { fitted: Fitted }) {
  const { bars, screen, scale } = fitted;
  const glyph = Math.min(bars.top * 0.32, 17 * scale);
  // Either side of the camera, a little below the middle, as the phones draw it.
  const third = { top: 0, bottom: 0, width: screen.w / 3, paddingTop: bars.top * 0.15 };
  return (
    <div className="absolute inset-x-0 top-0 text-black" style={{ height: bars.top }}>
      <div className="absolute left-0 flex items-center justify-center font-semibold" style={{ ...third, fontSize: glyph }}>
        9:41
      </div>
      <div className="absolute right-0 flex items-center justify-center" style={third}>
        <svg aria-hidden viewBox="0 0 68 12" style={{ height: glyph * 0.7 }}>
          {[3, 5.5, 8, 10.5].map((h, i) => (
            <rect key={i} x={i * 4.5} y={12 - h} width={3} height={h} rx={0.8} fill="currentColor" />
          ))}
          <path d="M29 4.2a10 10 0 0 1 13 0l-1.6 1.7a7.6 7.6 0 0 0-9.8 0zm2.7 2.9a6 6 0 0 1 7.6 0L35.5 11z" fill="currentColor" />
          <rect x={46.5} y={1.5} width={18} height={9} rx={2.5} fill="none" stroke="currentColor" strokeOpacity={0.4} />
          <rect x={48.5} y={3.5} width={14} height={5} rx={1.2} fill="currentColor" />
          <rect x={65.5} y={4.5} width={1.6} height={3} rx={0.8} fill="currentColor" fillOpacity={0.4} />
        </svg>
      </div>
      <Camera fitted={fitted} />
    </div>
  );
}

/** Turned: only the camera's side is kept from the page, as a phone's browser does in landscape. */
function SideBar({ fitted }: { fitted: Fitted }) {
  return (
    <div className="absolute inset-y-0 left-0" style={{ width: fitted.bars.left }}>
      <Camera fitted={fitted} />
    </div>
  );
}

/** Responsive's right and bottom edges and corner, outside the screen so they show. */
function Handles({ fitted, resize }: { fitted: Fitted; resize: NonNullable<Props["resize"]> }) {
  const s = fitted.screen;
  const grip = "absolute rounded-full bg-border-strong hover:bg-primary";
  return (
    <>
      <div aria-hidden onPointerDown={resize(1, 0)} className={`${grip} cursor-ew-resize`} style={{ left: s.x + s.w + 4, top: s.y + s.h / 2 - 20, width: 5, height: 40 }} />
      <div aria-hidden onPointerDown={resize(0, 1)} className={`${grip} cursor-ns-resize`} style={{ left: s.x + s.w / 2 - 20, top: s.y + s.h + 4, width: 40, height: 5 }} />
      <div aria-hidden onPointerDown={resize(1, 1)} className={`${grip} cursor-nwse-resize`} style={{ left: s.x + s.w + 4, top: s.y + s.h + 4, width: 8, height: 8 }} />
    </>
  );
}
