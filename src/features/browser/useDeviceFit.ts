import { type RefObject, useLayoutEffect, useState } from "react";
import { clampSide, type DeviceChoice, deviceOf } from "@/lib/browser/devices";
import { type Box, fit, type Fitted } from "@/lib/browser/fit";

/** Room around the body: Responsive's drag handles and a phone's side buttons sit in it. */
const PAD = 16;

const shift = (b: Box): Box => ({ ...b, x: b.x + PAD, y: b.y + PAD });

/**
 * A tab's device laid out in `area`, measured as it resizes: none until it has been measured.
 * Responsive's size follows a drag as it goes, and is kept (`onResized`) once it's let go.
 */
export function useDeviceFit(area: RefObject<HTMLElement | null>, choice: DeviceChoice | null, onResized: (size: { w: number; h: number }) => void) {
  const [room, setRoom] = useState<{ w: number; h: number } | null>(null);
  const [draft, setDraft] = useState<{ w: number; h: number } | null>(null);
  const on = !!choice;
  useLayoutEffect(() => {
    const el = area.current;
    if (!on || !el) return;
    const measure = () => setRoom((r) => (r?.w === el.clientWidth && r.h === el.clientHeight ? r : { w: el.clientWidth, h: el.clientHeight }));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [area, on]);

  const device = choice && deviceOf(draft ? { ...choice, ...draft } : choice);
  let fitted: Fitted | null = null;
  if (device && choice && room) {
    const f = fit(device, !!choice.rotated, { w: room.w - 2 * PAD, h: room.h - 2 * PAD });
    fitted = { ...f, frame: shift(f.frame), screen: shift(f.screen), page: shift(f.page) };
  }

  /** Dragging Responsive's right edge (`dx`), bottom edge (`dy`) or corner. */
  const resize = (dx: 0 | 1, dy: 0 | 1) => (e: React.PointerEvent<HTMLElement>) => {
    if (!device || !fitted || e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const [x0, y0, scale, from] = [e.clientX, e.clientY, fitted.scale, { w: device.w, h: device.h }];
    let size = from;
    // Twice the pointer's move: the screen stays centered, so each edge goes half the way.
    const move = (m: PointerEvent) => {
      if (m.buttons === 0) return end();
      size = { w: clampSide(from.w + (dx * 2 * (m.clientX - x0)) / scale), h: clampSide(from.h + (dy * 2 * (m.clientY - y0)) / scale) };
      setDraft(size);
    };
    // Let go, cancelled or the capture lost (each may follow another): kept once.
    let done = false;
    const end = () => {
      if (done) return;
      done = true;
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", end);
      el.removeEventListener("pointercancel", end);
      el.removeEventListener("lostpointercapture", end);
      if (size !== from) onResized(size);
      setDraft(null);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", end);
    el.addEventListener("pointercancel", end);
    el.addEventListener("lostpointercapture", end);
  };

  return { device, fitted, resize };
}
