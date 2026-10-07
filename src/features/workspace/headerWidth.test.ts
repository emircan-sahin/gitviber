import assert from "node:assert/strict";
import { test } from "node:test";
import { headerWidth } from "./headerWidth.ts";

function random(seed: number) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const rect = (left: number, width: number) => ({ left, right: left + width, width });

test("a header fits its tabs unscrolled beside its buttons", () => {
  // At 260px: 8px padding, a 300px row of tabs scrolled into 150px, 96px of buttons and 4px after them.
  const tabs = { getBoundingClientRect: () => rect(108, 150), scrollWidth: 300 };
  const buttons = { getBoundingClientRect: () => rect(260, 96) };
  const header = { getBoundingClientRect: () => rect(100, 260), firstElementChild: tabs, lastElementChild: buttons } as unknown as Element;
  assert.equal(headerWidth(header), 8 + 300 + 96 + 4);
  // Wider than it needs: the free space between tabs and buttons isn't counted.
  const wide = { getBoundingClientRect: () => rect(100, 600), firstElementChild: { getBoundingClientRect: () => rect(108, 300), scrollWidth: 300 }, lastElementChild: { getBoundingClientRect: () => rect(600, 96) } } as unknown as Element;
  assert.equal(headerWidth(wide), 408);
  // Collapsed to nothing, the buttons overflowing it: all but the right padding.
  const closed = { getBoundingClientRect: () => rect(100, 0), firstElementChild: { getBoundingClientRect: () => rect(108, 0), scrollWidth: 300 }, lastElementChild: { getBoundingClientRect: () => rect(108, 96) } } as unknown as Element;
  assert.equal(headerWidth(closed), 404);
});

test("1,000 random headers: the fit width holds the tabs unscrolled and the buttons, as a whole pixel", () => {
  const r = random(3);
  for (let i = 0; i < 1000; i++) {
    const left = r() * 2000;
    const width = r() < 0.1 ? 0 : 150 + r() * 500;
    const pad = 8 * (0.8 + r());
    const tabsWidth = 80 + r() * 400;
    const buttons = 24 + r() * 120;
    const tabsShown = Math.max(0, Math.min(tabsWidth, width - pad - buttons - 4));
    const after = width ? 4 : 0;
    const header = {
      getBoundingClientRect: () => rect(left, width),
      firstElementChild: { getBoundingClientRect: () => rect(left + pad, tabsShown), scrollWidth: Math.ceil(tabsWidth) },
      lastElementChild: { getBoundingClientRect: () => rect(width ? left + width - after - buttons : left + pad, buttons) },
    } as unknown as Element;
    const fit = headerWidth(header);
    assert.ok(Number.isInteger(fit));
    assert.ok(fit >= pad + Math.ceil(tabsWidth) + buttons, `${fit} for ${pad} + ${tabsWidth} + ${buttons}`);
    assert.ok(fit <= pad + Math.ceil(tabsWidth) + buttons + after + 1);
  }
});
