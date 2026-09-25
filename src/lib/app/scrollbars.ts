/**
 * Overlay scrollbars for every scrolling element in the app. WebKit's own are hidden (index.css):
 * styling them turns macOS's overlay bar into a classic one that takes layout width. Instead one
 * fixed layer draws a thin thumb over whatever the pointer is on, or what just scrolled, and fades
 * it out after, like macOS but in the app's colors. The code view (Monaco), the terminal (both
 * draw their own) and `data-scrollbar="none"` are left alone.
 */

type Axis = "x" | "y";

interface Bar {
  el: HTMLElement;
  /** Clipped to the part of `el` that's on screen, so a half scrolled-away `pre` doesn't draw over its neighbours. */
  box: HTMLDivElement;
  track: Record<Axis, HTMLDivElement>;
  thumb: Record<Axis, HTMLDivElement>;
  /** Track rects in client coordinates from the last layout, for hit testing on pointer moves. */
  rects: Partial<Record<Axis, DOMRect>>;
  scrolledAt: number;
  hiddenAt: number;
  observers: { disconnect(): void }[];
}

const SKIP = ".xterm, [data-scrollbar='none']";
/** The strip along the edge that belongs to the bar; index.css draws the thumb thinner inside it. */
const STRIP = 10;
const MIN_THUMB = 24;
/** How long a bar stays after the last scroll. */
const LINGER = 800;
/** The fade-out in index.css. */
const FADE = 200;

const bars = new Map<HTMLElement, Bar>();
const boxes = new WeakMap<Element, Bar>();
let layer: HTMLDivElement;
let hovered: HTMLElement[] = [];
let target: EventTarget | null = null;
let pointer: { x: number; y: number } | null = null;
let drag: { bar: Bar; axis: Axis; from: number; scroll: number } | null = null;
let frame = 0;
let dirty = false;
let linger = 0;

function schedule() {
  frame ||= requestAnimationFrame(render);
}

const relayout = () => {
  dirty = true;
  schedule();
};

function axes(el: HTMLElement) {
  const s = getComputedStyle(el);
  const scrolls = (o: string) => o === "auto" || o === "scroll";
  return { y: scrolls(s.overflowY) && el.scrollHeight - el.clientHeight > 1, x: scrolls(s.overflowX) && el.scrollWidth - el.clientWidth > 1 };
}

/** The elements that scroll under the pointer, innermost first. */
function scrollersAt(t: EventTarget | null) {
  const out: HTMLElement[] = [];
  let el = t instanceof Element ? t : null;
  el = el?.closest(SKIP)?.parentElement ?? el;
  for (; el && el !== document.body; el = el.parentElement) {
    if (el instanceof HTMLElement && !el.matches(SKIP)) {
      const a = axes(el);
      if (a.x || a.y) out.push(el);
    }
  }
  return out;
}

function ensure(el: HTMLElement): Bar {
  const had = bars.get(el);
  if (had) return had;
  const box = document.createElement("div");
  box.className = "sb-box";
  const part = (axis: Axis) => {
    const track = document.createElement("div");
    track.className = "sb-track";
    track.dataset.axis = axis;
    const thumb = document.createElement("div");
    thumb.className = "sb-thumb";
    track.append(thumb);
    box.append(track);
    return [track, thumb] as const;
  };
  const [ty, hy] = part("y");
  const [tx, hx] = part("x");
  const ro = new ResizeObserver(relayout);
  ro.observe(el);
  const mo = new MutationObserver(relayout);
  mo.observe(el, { childList: true, subtree: true });
  const bar: Bar = { el, box, track: { y: ty, x: tx }, thumb: { y: hy, x: hx }, rects: {}, scrolledAt: 0, hiddenAt: 0, observers: [ro, mo] };
  bars.set(el, bar);
  boxes.set(box, bar);
  layer.append(box);
  return bar;
}

function remove(bar: Bar) {
  bar.observers.forEach((o) => o.disconnect());
  bar.box.remove();
  bars.delete(bar.el);
}

/** The part of `el` not cut off by an ancestor that clips, or the window. */
function visibleRect(el: HTMLElement, r: DOMRect) {
  let { left, top, right, bottom } = r;
  for (let p = el.parentElement; p && p !== document.body; p = p.parentElement) {
    const s = getComputedStyle(p);
    if (s.overflowX === "visible" && s.overflowY === "visible") continue;
    const q = p.getBoundingClientRect();
    left = Math.max(left, q.left);
    top = Math.max(top, q.top);
    right = Math.min(right, q.right);
    bottom = Math.min(bottom, q.bottom);
  }
  right = Math.min(right, innerWidth);
  bottom = Math.min(bottom, innerHeight);
  return right > left && bottom > top ? new DOMRect(Math.max(left, 0), Math.max(top, 0), right - Math.max(left, 0), bottom - Math.max(top, 0)) : null;
}

function place(bar: Bar) {
  const { el, box } = bar;
  const r = el.getBoundingClientRect();
  const clip = visibleRect(el, r);
  const a = axes(el);
  if (!clip || !(a.x || a.y)) {
    delete box.dataset.on;
    bar.rects = {};
    return;
  }
  Object.assign(box.style, { left: `${clip.left}px`, top: `${clip.top}px`, width: `${clip.width}px`, height: `${clip.height}px` });
  // The padding box: borders don't scroll.
  const left = r.left + el.clientLeft;
  const top = r.top + el.clientTop;
  const { clientWidth: w, clientHeight: h } = el;
  const set = (axis: Axis, on: boolean, rect: DOMRect, view: number, content: number, scroll: number) => {
    const track = bar.track[axis];
    track.hidden = !on;
    if (!on) return void delete bar.rects[axis];
    bar.rects[axis] = rect;
    Object.assign(track.style, { left: `${rect.left - clip.left}px`, top: `${rect.top - clip.top}px`, width: `${rect.width}px`, height: `${rect.height}px` });
    const length = axis === "y" ? rect.height : rect.width;
    const size = Math.min(length, Math.max(MIN_THUMB, (length * view) / content));
    const max = content - view;
    const at = ((length - size) * Math.min(max, Math.max(0, scroll))) / max;
    Object.assign(bar.thumb[axis].style, axis === "y" ? { top: `${at}px`, height: `${size}px` } : { left: `${at}px`, width: `${size}px` });
  };
  set("y", a.y, new DOMRect(left + w - STRIP, top, STRIP, h - (a.x ? STRIP : 0)), h, el.scrollHeight, el.scrollTop);
  set("x", a.x, new DOMRect(left, top + h - STRIP, w - (a.y ? STRIP : 0), STRIP), w, el.scrollWidth, el.scrollLeft);
  box.dataset.on = "";
}

const inside = (p: { x: number; y: number }, r?: DOMRect) => !!r && p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom;

function render() {
  frame = 0;
  const now = performance.now();
  // Over the layer itself (a thumb, a track) the pointer is still over what that bar belongs to.
  if (target !== null && !(target instanceof Node && layer.contains(target))) {
    hovered = scrollersAt(target);
    target = null;
    hovered.forEach(ensure);
  }
  const relayoutAll = dirty;
  dirty = false;
  for (const bar of bars.values()) {
    const shown = bar.el.isConnected && (hovered.includes(bar.el) || now - bar.scrolledAt < LINGER || drag?.bar === bar);
    if (shown) {
      if (relayoutAll || !("on" in bar.box.dataset)) place(bar);
      // Near the edge the bar widens and takes clicks, like macOS; elsewhere clicks go through to the content.
      for (const axis of ["x", "y"] as const) bar.track[axis].toggleAttribute("data-hot", drag?.bar === bar ? drag.axis === axis : !!pointer && inside(pointer, bar.rects[axis]));
      bar.hiddenAt = 0;
    } else if ("on" in bar.box.dataset) {
      delete bar.box.dataset.on;
      bar.hiddenAt = now;
      setTimeout(schedule, FADE);
    } else if (now - bar.hiddenAt >= FADE) remove(bar);
  }
}

function startDrag(e: PointerEvent) {
  const track = e.target instanceof Element ? e.target.closest<HTMLElement>(".sb-track") : null;
  const bar = track && boxes.get(track.parentElement!);
  if (!bar || !track || e.button !== 0) return;
  // Keeps focus where it is and, since the layer is outside any open dialog, stops Radix taking this as a click outside.
  e.preventDefault();
  e.stopPropagation();
  const axis = track.dataset.axis as Axis;
  const { el } = bar;
  const at = axis === "y" ? e.clientY : e.clientX;
  const thumb = bar.thumb[axis];
  // On the track, jump so the thumb is centred under the pointer, then drag from there.
  if (e.target !== thumb) {
    const t = thumb.getBoundingClientRect();
    const d = at - (axis === "y" ? t.top + t.height / 2 : t.left + t.width / 2);
    el[axis === "y" ? "scrollTop" : "scrollLeft"] += d * ratio(bar, axis);
  }
  drag = { bar, axis, from: at, scroll: axis === "y" ? el.scrollTop : el.scrollLeft };
  track.setPointerCapture(e.pointerId);
  thumb.dataset.active = "";
  schedule();
}

/** Scroll distance per pixel of thumb movement. */
function ratio(bar: Bar, axis: Axis) {
  const { el } = bar;
  const track = bar.rects[axis];
  const thumb = bar.thumb[axis].getBoundingClientRect();
  const free = axis === "y" ? track!.height - thumb.height : track!.width - thumb.width;
  const max = axis === "y" ? el.scrollHeight - el.clientHeight : el.scrollWidth - el.clientWidth;
  return free > 0 ? max / free : 0;
}

function moveDrag(e: PointerEvent) {
  if (!drag) return;
  const { bar, axis, from, scroll } = drag;
  bar.el[axis === "y" ? "scrollTop" : "scrollLeft"] = scroll + ((axis === "y" ? e.clientY : e.clientX) - from) * ratio(bar, axis);
}

function endDrag() {
  if (!drag) return;
  delete drag.bar.thumb[drag.axis].dataset.active;
  drag = null;
  schedule();
}

let installed = false;

export function installScrollbars() {
  if (installed) return;
  installed = true;
  layer = document.createElement("div");
  layer.className = "sb-layer";
  document.body.append(layer);

  document.addEventListener(
    "scroll",
    (e) => {
      const el = e.target;
      // Any scroll can move the bars on screen (an outer list carrying an inner `pre`).
      dirty = true;
      if (el instanceof HTMLElement && !el.closest(SKIP)) {
        ensure(el).scrolledAt = performance.now();
        clearTimeout(linger);
        linger = setTimeout(schedule, LINGER);
      }
      schedule();
    },
    { capture: true, passive: true },
  );
  document.addEventListener(
    "pointermove",
    (e) => {
      target = e.target;
      pointer = { x: e.clientX, y: e.clientY };
      schedule();
    },
    { capture: true, passive: true },
  );
  // Left the window.
  document.addEventListener("pointerout", (e) => {
    if (e.relatedTarget) return;
    target = null;
    hovered = [];
    pointer = null;
    schedule();
  });
  addEventListener("resize", relayout);
  // An image that finished loading changes how far its page scrolls.
  document.addEventListener("load", relayout, true);

  layer.addEventListener("pointerdown", startDrag);
  layer.addEventListener("pointermove", moveDrag);
  layer.addEventListener("pointerup", endDrag);
  layer.addEventListener("pointercancel", endDrag);
  layer.addEventListener("lostpointercapture", endDrag);
  // A wheel over a thumb or track would otherwise land on the layer, not the element it scrolls.
  layer.addEventListener(
    "wheel",
    (e) => {
      const bar = e.target instanceof Element ? boxes.get(e.target.closest(".sb-box")!) : undefined;
      if (!bar) return;
      e.preventDefault();
      const unit = e.deltaMode === WheelEvent.DOM_DELTA_LINE ? 16 : e.deltaMode === WheelEvent.DOM_DELTA_PAGE ? bar.el.clientHeight : 1;
      bar.el.scrollBy(e.deltaX * unit, e.deltaY * unit);
    },
    { passive: false },
  );
}
