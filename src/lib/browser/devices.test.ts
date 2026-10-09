import assert from "node:assert/strict";
import { test } from "node:test";
import { DEFAULT_DEVICE, DEVICES, deviceOf, isDeviceChoice, RESPONSIVE, RESPONSIVE_MAX, RESPONSIVE_MIN } from "./devices.ts";

test("every device is a screen a page can lay out on", () => {
  assert.ok(DEVICES.length >= 11);
  assert.equal(new Set(DEVICES.map((d) => d.name)).size, DEVICES.length, "names are unique");
  for (const d of DEVICES) {
    assert.ok(d.w > 0 && d.h > 0 && d.h >= d.w, `${d.name}: portrait, positive`);
    assert.ok(d.dpr >= 1 && d.dpr <= 4, `${d.name}: dpr`);
    assert.match(d.ua, /^Mozilla\/5\.0 \(/, d.name);
    assert.ok(d.radius >= 0 && d.radius < Math.min(d.w, d.h) / 2, `${d.name}: radius`);
    assert.ok(d.bezel > 0, `${d.name}: bezel`);
    for (const inset of Object.values(d.safe)) assert.ok(inset >= 0 && inset < d.h / 4, `${d.name}: safe area`);
    const c = d.cutout;
    if (c) {
      assert.ok(c.w > 0 && c.h > 0 && c.x >= 0 && c.y >= 0, `${d.name}: cutout`);
      assert.ok(c.x + c.w <= d.w && c.y + c.h <= d.h, `${d.name}: cutout inside the screen`);
      assert.ok(c.y + c.h <= d.safe.top, `${d.name}: cutout inside the top safe area`);
      // DevTools rounds a pill 37 high to 19.
      assert.ok(c.r <= Math.ceil(Math.min(c.w, c.h) / 2), `${d.name}: cutout radius`);
    }
  }
  assert.ok(deviceOf(DEFAULT_DEVICE), "the default is one of them");
});

test("Responsive is a bare screen of its own size, kept in bounds", () => {
  const r = deviceOf({ name: RESPONSIVE, w: 500.4, h: 50 })!;
  assert.deepEqual([r.w, r.h, r.bezel, r.radius, r.dpr, r.ua, r.cutout], [500, RESPONSIVE_MIN, 0, 0, 0, "", null]);
  assert.equal(deviceOf({ name: RESPONSIVE, w: 99_999 })!.w, RESPONSIVE_MAX);
  assert.equal(deviceOf({ name: "Nokia 3310" }), null);
});

test("a stored device choice is checked before a tab takes it", () => {
  for (const ok of [{ name: "iPhone 16" }, { name: "iPhone 16", rotated: true }, { name: RESPONSIVE, w: 320, h: 640 }]) assert.equal(isDeviceChoice(ok), true, JSON.stringify(ok));
  for (const no of [null, "iPhone", {}, { name: 1 }, { name: "x", rotated: "yes" }, { name: "x", w: -1 }, { name: "x", h: Number.NaN }, { name: "x", w: "9" }]) {
    assert.equal(isDeviceChoice(no), false, JSON.stringify(no));
  }
});
