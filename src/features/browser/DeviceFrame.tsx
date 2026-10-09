import { type ReactNode, type RefObject, useEffect, useLayoutEffect, useRef, useState } from "react";
import { type Device, RESPONSIVE_MAX, RESPONSIVE_MIN } from "@/lib/browser/devices";
import { fit, type Fitted } from "@/lib/browser/fit";

/** Room around the body: Responsive's drag handles and a phone's side buttons sit in it. */
const PAD = 16;

interface Props {
  device: Device;
  rotated: boolean;
  /** The screen's element: the page's native view is laid exactly over it. */
  screen: RefObject<HTMLDivElement | null>;
  onFit: (fitted: Fitted) => void;
  /** Responsive: dragging an edge resizes it, in device CSS px. */
  onResize?: (size: { w: number; h: number }) => void;
  /** What shows in the screen while the native view doesn't (a picture, a message). */
  children?: ReactNode;
}

/**
 * A device's body, drawn here from devices.json (no maker's artwork), centered in the tab and
 * shrunk to fit, with its screen inside. The native view covers the screen; the app page can't
 * draw over it, so the body, buttons and handles all sit outside it.
 */
export function DeviceFrame({ device, rotated, screen, onFit, onResize, children }: Props) {
  const room = useRef<HTMLDivElement>(null);
  const [area, setArea] = useState({ w: 0, h: 0 });
  useLayoutEffect(() => {
    const el = room.current;
    if (!el) return;
    const measure = () => setArea((a) => (a.w === el.clientWidth && a.h === el.clientHeight ? a : { w: el.clientWidth, h: el.clientHeight }));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  const fitted = fit(device, rotated, area);
  const key = JSON.stringify(fitted);
  useEffect(() => onFit(fitted), [key]);

  const { frame, screen: s, scale } = fitted;
  const body = (device.radius + device.bezel) * scale;
  const phone = device.platform === "ios" || device.platform === "android";
  // Volume on the left, power on the right, as on most phones; along the long side either way up.
  const buttons = phone
    ? rotated
      ? [
          { x: frame.w * 0.62, y: -3 * scale, w: 50 * scale, h: 3 * scale },
          { x: frame.w * 0.45, y: -3 * scale, w: 50 * scale, h: 3 * scale },
          { x: frame.w * 0.55, y: frame.h, w: 70 * scale, h: 3 * scale },
        ]
      : [
          { x: -3 * scale, y: frame.h * 0.2, w: 3 * scale, h: 50 * scale },
          { x: -3 * scale, y: frame.h * 0.3, w: 3 * scale, h: 50 * scale },
          { x: frame.w, y: frame.h * 0.25, w: 3 * scale, h: 70 * scale },
        ]
    : [];

  return (
    <div ref={room} className="absolute select-none" style={{ inset: PAD }}>
      {area.w > 0 && (
        <>
          <svg aria-hidden className="absolute overflow-visible" style={{ left: frame.x, top: frame.y, width: frame.w, height: frame.h }}>
            {buttons.map((b, i) => (
              <rect key={i} {...b} rx={1.5 * scale} className="fill-border-strong" />
            ))}
            {device.bezel > 0 && <rect x={0.5} y={0.5} width={frame.w - 1} height={frame.h - 1} rx={body} className="fill-elevated stroke-border-strong" />}
          </svg>
          <div ref={screen} tabIndex={-1} className="absolute overflow-hidden bg-background outline-none" style={{ left: s.x, top: s.y, width: s.w, height: s.h, borderRadius: fitted.radius }}>
            {children}
          </div>
          {onResize && <Handles fitted={fitted} device={device} onResize={onResize} />}
        </>
      )}
    </div>
  );
}

/** Responsive's edges: dragged, the screen takes the new size (in device CSS px). */
function Handles({ fitted, device, onResize }: { fitted: Fitted; device: Device; onResize: (size: { w: number; h: number }) => void }) {
  const { screen: s, scale } = fitted;
  const drag = (dx: number, dy: number) => (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const [x0, y0] = [e.clientX, e.clientY];
    const clamp = (v: number) => Math.round(Math.min(RESPONSIVE_MAX, Math.max(RESPONSIVE_MIN, v)));
    const move = (m: PointerEvent) =>
      // Twice the pointer's move: the screen stays centered, so each edge goes half the way.
      onResize({ w: clamp(device.w + (dx * 2 * (m.clientX - x0)) / scale), h: clamp(device.h + (dy * 2 * (m.clientY - y0)) / scale) });
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  };
  const grip = "absolute rounded-full bg-border-strong hover:bg-primary";
  return (
    <>
      <div aria-hidden onPointerDown={drag(1, 0)} className={`${grip} cursor-ew-resize`} style={{ left: s.x + s.w + 4, top: s.y + s.h / 2 - 20, width: 5, height: 40 }} />
      <div aria-hidden onPointerDown={drag(0, 1)} className={`${grip} cursor-ns-resize`} style={{ left: s.x + s.w / 2 - 20, top: s.y + s.h + 4, width: 40, height: 5 }} />
      <div aria-hidden onPointerDown={drag(1, 1)} className={`${grip} cursor-nwse-resize`} style={{ left: s.x + s.w + 4, top: s.y + s.h + 4, width: 8, height: 8 }} />
    </>
  );
}
