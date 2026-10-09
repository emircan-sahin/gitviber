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
  assert.deepEqual(f.viewport, { w: 400, h: 800 });
  assert.equal(f.radius, 40);
  assert.deepEqual(f.cutout, { x: 150, y: 10, w: 100, h: 30, r: 15 });
});

test("one that doesn't shrinks by its tighter side, the page still laid out at its width", () => {
  const f = fit(phone, false, { w: 1000, h: 410 });
  assert.equal(f.scale, 0.5);
  assert.deepEqual(f.screen, { x: 400, y: 5, w: 200, h: 400 });
  assert.deepEqual(f.viewport, { w: 400, h: 800 });
  assert.equal(f.radius, 20);
  assert.deepEqual(f.cutout, { x: 75, y: 5, w: 50, h: 15, r: 7.5 });
});

test("rotated, the sides swap and the cutout and safe area turn to the left", () => {
  const f = fit(phone, true, { w: 2000, h: 2000 });
  assert.deepEqual(f.viewport, { w: 800, h: 400 });
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
        assert.ok(f.screen.w <= f.frame.w && f.screen.h <= f.frame.h, d.name);
        if (f.cutout) assert.ok(f.cutout.x + f.cutout.w <= f.screen.w + 1e-9 && f.cutout.y + f.cutout.h <= f.screen.h + 1e-9, `${d.name} ${rotated}`);
      }
    }
  }
});
