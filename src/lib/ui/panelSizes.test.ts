import assert from "node:assert/strict";
import { test } from "node:test";
import { headerWidth, initialSize, openSize, remembered } from "./panelSizes.ts";

test("a layout is remembered in pixels, a closed panel with the size it reopens at", () => {
  const ids = ["list", "files"];
  // 1500px: 20% is 300px, 16% is 240px; the viewer isn't a fixed panel.
  const open = remembered({}, { list: 20, viewer: 64, files: 16 }, 1500, ids);
  assert.deepEqual(open, { list: { size: 300 }, files: { size: 240 } });
  const closed = remembered(open, { list: 0, viewer: 84, files: 16 }, 1500, ids);
  assert.deepEqual(closed, { list: { size: 300, collapsed: true }, files: { size: 240 } });
  // Never opened this run, closed: nothing to reopen at but the base.
  assert.deepEqual(remembered({}, { list: 0, viewer: 100 }, 1500, ["list"]), { list: { collapsed: true } });
  // The terminal not shown: its size stays as it was.
  assert.deepEqual(remembered({ terminal: { size: 280 } }, { editor: 100 }, 900, ["terminal"]), { terminal: { size: 280 } });
});

test("a panel opens as it was left, else at its base", () => {
  assert.equal(initialSize(undefined, 320), 320);
  assert.equal(initialSize({ size: 410 }, 320), 410);
  assert.equal(initialSize({ size: 410, collapsed: true }, 320), 0);
  assert.equal(openSize({ size: 410, collapsed: true }, 320), 410);
  assert.equal(openSize({ collapsed: true }, "35"), "35");
  // Whatever else an older build stored.
  assert.equal(openSize({ size: Number.NaN }, 320), 320);
  assert.equal(openSize({ size: 0 }, 320), 320);
});

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
