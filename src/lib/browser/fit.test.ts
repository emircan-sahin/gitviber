import assert from "node:assert/strict";
import { test } from "node:test";
import { type Device, DEVICES, deviceOf, RESPONSIVE } from "./devices.ts";
import { fit } from "./fit.ts";

const phone: Device = {
  name: "Test Phone",
  w: 400,
  h: 800,
  dpr: 3,
  ua: "Mozilla/5.0 (Test)",
  radius: 40,
  safe: { top: 50, right: 0, bottom: 30, left: 0 },
  cutout: { type: "island", x: 150, y: 10, w: 100, h: 30, r: 15 },
  bezel: 10,
  platform: "ios",
};

test("a device that fits shows at its own size, centered", () => {
  const f = fit(phone, false, { w: 1000, h: 1000 });
  assert.equal(f.scale, 1);
  assert.deepEqual(f.frame, { x: 290, y: 90, w: 420, h: 820 });
  assert.deepEqual(f.screen, { x: 300, y: 100, w: 400, h: 800 });
  assert.deepEqual(f.size, { w: 400, h: 800 });
  assert.equal(f.radius, 40);
  assert.deepEqual(f.cutout, { x: 150, y: 10, w: 100, h: 30, r: 15 });
});

test("the page gets the screen less its status bar and home indicator, rounded where it meets the screen's corners", () => {
  const f = fit(phone, false, { w: 1000, h: 1000 });
  assert.deepEqual(f.bars, { top: 50, left: 0, bottom: 30 });
  assert.deepEqual(f.viewport, { w: 400, h: 720 });
  assert.deepEqual(f.page, { x: 300, y: 150, w: 400, h: 720 });
  assert.deepEqual(f.corners, { topLeft: false, topRight: false, bottomRight: false, bottomLeft: false });
  // Without a home indicator the page reaches the bottom corners.
  const pixel = fit({ ...phone, safe: { ...phone.safe, bottom: 0 } }, false, { w: 1000, h: 1000 });
  assert.deepEqual(pixel.corners, { topLeft: false, topRight: false, bottomRight: true, bottomLeft: true });
  // Turned: only the camera's side has a bar, and the page has the right corners.
  const side = fit(phone, true, { w: 2000, h: 2000 });
  assert.deepEqual(side.bars, { top: 0, left: 50, bottom: 0 });
  assert.deepEqual(side.viewport, { w: 750, h: 400 });
  assert.deepEqual(side.corners, { topLeft: false, topRight: true, bottomRight: true, bottomLeft: false });
});

test("the page lays out at its viewport's width once its rect is rounded to half points", () => {
  const half = (v: number) => Math.round(v * 2) / 2;
  const d = DEVICES.find((x) => x.name === "iPhone 16 Pro")!;
  const bad: string[] = [];
  for (const ui of [0.9, 1, 1.1, 1.25]) {
    for (let h = 300; h < 900; h++) {
      const f = fit(d, false, { w: 2000, h });
      // As browser/macos.rs zooms it: the placed width over the viewport's.
      const placed = half(f.page.w * ui);
      const zoom = placed / f.viewport.w;
      if (Math.abs(placed / zoom - d.w) > 1e-9 || Math.abs(zoom - f.scale * ui) > 0.01) bad.push(`ui ${ui}, area h ${h}`);
    }
  }
  assert.deepEqual(bad.slice(0, 3), []);
});

test("the screen never leaves the tab's area", () => {
  const wide = deviceOf({ name: RESPONSIVE, w: 3000, h: 800 })!;
  const f = fit(wide, false, { w: 120, h: 400 });
  assert.ok(f.screen.x >= 0 && f.screen.x + f.screen.w <= 120, JSON.stringify(f.screen));
  assert.equal(f.fits, false, "too small to read: the tab says so");
});

test("one that doesn't shrinks by its tighter side, the page still laid out at its width", () => {
  const f = fit(phone, false, { w: 1000, h: 410 });
  assert.equal(f.scale, 0.5);
  assert.deepEqual(f.screen, { x: 400, y: 5, w: 200, h: 400 });
  assert.deepEqual(f.viewport, { w: 400, h: 720 });
  assert.equal(f.radius, 20);
  assert.deepEqual(f.cutout, { x: 75, y: 5, w: 50, h: 15, r: 7.5 });
});

test("rotated, the sides swap and the cutout and safe area turn to the left", () => {
  const f = fit(phone, true, { w: 2000, h: 2000 });
  assert.deepEqual(f.size, { w: 800, h: 400 });
  assert.deepEqual(f.cutout, { x: 10, y: 150, w: 30, h: 100, r: 15 });
  assert.deepEqual(f.safe, { top: 0, right: 30, bottom: 0, left: 50 });
  const c = f.cutout!;
  assert.ok(c.x + c.w <= f.screen.w && c.y + c.h <= f.screen.h, "inside the screen");
});

test("every device's cutout stays inside its screen at any size and either way up", () => {
  for (const d of [...DEVICES, deviceOf({ name: RESPONSIVE, w: 320, h: 2000 })!]) {
    for (const rotated of [false, true]) {
      for (const area of [{ w: 300, h: 300 }, { w: 1600, h: 900 }, { w: 1, h: 1 }]) {
        const f = fit(d, rotated, area);
        assert.ok(f.scale > 0 && f.scale <= 1, d.name);
        assert.ok(f.frame.x >= -1e-9 && f.frame.y >= -1e-9 && f.frame.x + f.frame.w <= area.w + 1e-9, `${d.name}: inside the area`);
        assert.ok(f.screen.w <= f.frame.w && f.screen.h <= f.frame.h, d.name);
        if (f.cutout) assert.ok(f.cutout.x + f.cutout.w <= f.screen.w + 1e-9 && f.cutout.y + f.cutout.h <= f.screen.h + 1e-9, `${d.name} ${rotated}`);
      }
    }
  }
});

test("no area, a negative one or a sliver still gives a finite, centered fit, too small to show", () => {
  for (const area of [{ w: 0, h: 0 }, { w: -50, h: 300 }, { w: 300, h: -1 }, { w: 2, h: 2 }]) {
    const f = fit(phone, false, area);
    assert.equal(f.fits, false, JSON.stringify(area));
    assert.ok(f.scale >= 0 && f.scale < 0.15, JSON.stringify(area));
    for (const v of [f.frame.x, f.frame.y, f.frame.w, f.frame.h, f.screen.x, f.screen.y, f.radius]) assert.ok(Number.isFinite(v), JSON.stringify(area));
    assert.ok(Math.abs(f.frame.x - (area.w - f.frame.w) / 2) < 1e-9);
    assert.deepEqual(f.size, { w: 400, h: 800 }, "the screen keeps the device's size");
  }
});

test("a big area never enlarges, and the screen keeps the device's shape at any scale", () => {
  for (const d of [...DEVICES, deviceOf({ name: RESPONSIVE, w: 3000, h: 200 })!, deviceOf({ name: RESPONSIVE, w: 200, h: 3000 })!]) {
    for (const rotated of [false, true]) {
      assert.equal(fit(d, rotated, { w: 100_000, h: 100_000 }).scale, 1, d.name);
      for (let i = 1; i <= 60; i++) {
        const area = { w: 37 * i, h: 23 * i + 11 };
        const f = fit(d, rotated, area);
        const ratio = f.size.w / f.size.h;
        assert.ok(Math.abs(f.screen.w / f.screen.h - ratio) < 1e-9, `${d.name} ${rotated} ${i}`);
        assert.ok(Math.abs(f.screen.w - f.size.w * f.scale) < 1e-9);
        assert.ok(Math.abs(f.page.w - f.viewport.w * f.scale) < 1e-9 && f.page.w <= f.screen.w + 1e-9);
        // It fits whole, centered, however small.
        assert.ok(f.frame.x >= -1e-9 && f.frame.y >= -1e-9, `${d.name} ${rotated} ${JSON.stringify(area)}`);
        assert.ok(f.frame.x + f.frame.w <= area.w + 1e-9 && f.frame.y + f.frame.h <= area.h + 1e-9);
        assert.ok(f.screen.x >= f.frame.x && f.screen.x + f.screen.w <= f.frame.x + f.frame.w + 1e-9);
      }
    }
  }
});

test("turned, a device's safe area and cutout go where its top went, and none is lost", () => {
  for (const d of DEVICES) {
    const up = fit(d, false, { w: 5000, h: 5000 });
    const side = fit(d, true, { w: 5000, h: 5000 });
    assert.deepEqual(side.size, { w: d.h, h: d.w }, d.name);
    assert.deepEqual(side.safe, { top: d.safe.right, right: d.safe.bottom, bottom: d.safe.left, left: d.safe.top }, d.name);
    const sum = (s: typeof d.safe) => s.top + s.right + s.bottom + s.left;
    assert.equal(sum(side.safe), sum(up.safe));
    if (!d.cutout) {
      assert.equal(side.cutout, null);
      continue;
    }
    const c = side.cutout!;
    // The camera sits in the left safe area now, the same size turned, the same distance in.
    assert.deepEqual([c.w, c.h, c.r], [d.cutout.h, d.cutout.w, d.cutout.r], d.name);
    assert.equal(c.x, d.cutout.y);
    assert.ok(c.x + c.w <= side.safe.left, `${d.name}: inside the left safe area`);
    assert.ok(Math.abs(c.y + c.h / 2 - d.w / 2) <= 1, `${d.name}: a centered camera stays centered`);
  }
});
