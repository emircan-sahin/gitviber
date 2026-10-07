import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// useTabStrip without a DOM: "react" is a stand-in whose ref is the strip under test and whose
// layout effect runs at once.
register(
  "data:text/javascript," +
    encodeURIComponent(`
export async function resolve(spec, ctx, next) {
  if (spec === "react") return { url: "data:text/javascript,export const useRef = () => globalThis.__strip; export const useLayoutEffect = (f) => f();", shortCircuit: true };
  return next(spec, ctx);
}`),
);
const { useTabStrip } = await import("./useTabStrip.ts");

const rect = (left: number, width: number) => ({ left, right: left + width, width });

/** A strip `width` wide at x=100, scrolled to `scrollLeft`, its tabs laid out from `widths`, `selected` the one on show. */
function strip(width: number, widths: number[], selected: number | null, scrollLeft = 0) {
  const el = {
    scrollLeft,
    getBoundingClientRect: () => rect(100, width),
    querySelector: () =>
      selected === null
        ? null
        : { getBoundingClientRect: () => rect(100 - el.scrollLeft + widths.slice(0, selected).reduce((a, b) => a + b, 0), widths[selected]) },
  };
  (globalThis as Record<string, unknown>).__strip = { current: el };
  return el;
}

test("the tab on show scrolls into view, the strip only by as much as it needs", () => {
  const tabs = [120, 120, 120, 120, 120];
  // Already in view: left alone.
  let s = strip(300, tabs, 1, 0);
  useTabStrip(1);
  assert.equal(s.scrollLeft, 0);
  // Off to the right: its right edge comes to the strip's.
  s = strip(300, tabs, 4, 0);
  useTabStrip(4);
  assert.equal(s.scrollLeft, 600 - 300);
  // Off to the left: its left edge comes to the strip's.
  s = strip(300, tabs, 0, 250);
  useTabStrip(0);
  assert.equal(s.scrollLeft, 0);
  // Wider than the strip: its start shows.
  s = strip(100, [50, 400], 1, 0);
  useTabStrip(1);
  assert.equal(s.scrollLeft, 50);
  // Nothing on show, or a collapsed panel (all zero): nothing moves.
  s = strip(300, tabs, null, 40);
  useTabStrip(null);
  assert.equal(s.scrollLeft, 40);
  s = strip(0, [0, 0], 1, 0);
  useTabStrip(1);
  assert.equal(s.scrollLeft, 0);
});

test("a wheel scrolls the strip sideways; a sideways swipe is left to the trackpad", () => {
  const s = strip(300, [120, 120, 120, 120], null);
  const { onWheel } = useTabStrip(null);
  let prevented = 0;
  const wheel = (deltaX: number, deltaY: number) => onWheel({ deltaX, deltaY, currentTarget: s, preventDefault: () => prevented++ } as never);
  wheel(0, 40);
  assert.equal(s.scrollLeft, 40);
  wheel(0, -15);
  assert.equal(s.scrollLeft, 25);
  // Mostly sideways (a trackpad, shift+wheel): the browser scrolls it natively.
  wheel(30, 10);
  wheel(-30, 0);
  assert.equal(s.scrollLeft, 25);
  // React's wheel listener is passive: preventDefault would only warn.
  assert.equal(prevented, 0);
});
