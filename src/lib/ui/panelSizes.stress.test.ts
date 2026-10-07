import assert from "node:assert/strict";
import { test } from "node:test";
import { initialSize, openSize, type PanelSizes, remembered, toPixels } from "./panelSizes.ts";

function random(seed: number) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const GARBAGE: unknown[] = [undefined, null, {}, { size: 0 }, { size: -40 }, { size: Number.NaN }, { size: "320" }, { size: null }, { collapsed: "yes" }, { size: 410 }, { size: 410, collapsed: true }, { collapsed: true }];

test("whatever was stored, a panel opens at a positive size or its base", () => {
  for (const saved of GARBAGE) {
    for (const base of [320, "35"]) {
      const open = openSize(saved as never, base);
      assert.ok(open === base || (typeof open === "number" && open > 0 && Number.isFinite(open)), `${JSON.stringify(saved)} -> ${open}`);
      const initial = initialSize(saved as never, base);
      assert.ok(initial === 0 || initial === open, `${JSON.stringify(saved)} mounts at ${initial}`);
      assert.equal(initial === 0, (saved as { collapsed?: unknown } | undefined)?.collapsed === true);
    }
  }
});

test("1,000 random layouts: remembered keeps sizes in pixels, closed ones with their open size, absent ones as they were", () => {
  const r = random(7);
  const ids = ["list", "files", "terminal"];
  let sizes: PanelSizes = {};
  for (let i = 0; i < 1000; i++) {
    const total = 400 + Math.floor(r() * 4800);
    const layout: Record<string, number> = { viewer: 0 };
    let left = 100;
    for (const id of ids) {
      const roll = r();
      if (roll < 0.2) continue;
      layout[id] = roll < 0.4 ? 0 : Math.min(left, r() * 45);
      left -= layout[id];
    }
    layout.viewer = left;
    const next = remembered(sizes, layout, total, ids);
    assert.ok(!("viewer" in next), "only the fixed panels");
    for (const id of ids) {
      const share = layout[id];
      if (share === undefined) assert.deepEqual(next[id], sizes[id], `${id} not shown`);
      else if (share > 0) {
        assert.deepEqual(next[id], { size: toPixels(share, total) });
        assert.ok(Number.isInteger(next[id].size));
      } else {
        assert.equal(next[id].collapsed, true);
        assert.equal(next[id].size, sizes[id]?.size, `${id} keeps the size it reopens at`);
      }
    }
    // Saving the same layout again changes nothing.
    assert.deepEqual(remembered(next, layout, total, ids), next);
    // Through storage and back.
    sizes = JSON.parse(JSON.stringify(next));
    assert.deepEqual(sizes, next);
  }
});

test("pixels to share and back round-trip within a pixel at any group size", () => {
  const r = random(11);
  for (let i = 0; i < 1000; i++) {
    const total = 200 + Math.floor(r() * 5000);
    const px = Math.floor(r() * total);
    // The library keeps shares to 3 decimals.
    const share = Number(((px / total) * 100).toFixed(3));
    assert.ok(Math.abs(toPixels(share, total) - px) <= 1, `${px}px of ${total}`);
  }
});
