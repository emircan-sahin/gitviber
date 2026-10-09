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

test("each device's user agent names its platform, and its sizes are whole CSS px", () => {
  const says: Record<string, RegExp> = {
    ios: /\(iPhone; CPU iPhone OS \d+_\d+ like Mac OS X\).*Mobile\/\w+ Safari/,
    // iPadOS asks for desktop sites on the larger iPads, as DevTools' iPad Pro does.
    ipados: /\((iPad; CPU OS \d+_\d+ like Mac OS X|Macintosh; Intel Mac OS X 10_15_7)\)/,
    android: /\(Linux; Android \d+; [^)]+\) AppleWebKit\/537\.36 .*Chrome\/\d+\.0\.0\.0 Mobile Safari\/537\.36$/,
  };
  for (const d of DEVICES) {
    assert.match(d.ua, says[d.platform], d.name);
    assert.ok(!d.ua.includes("%s"), `${d.name}: DevTools' version placeholder filled in`);
    for (const v of [d.w, d.h]) assert.ok(Number.isInteger(v), d.name);
    // The phones and tablets ship at these ratios; a typo (25 for 2.5) shows here.
    assert.ok([2, 2.625, 3, 3.5].includes(d.dpr), `${d.name}: ${d.dpr}`);
  }
});

test("Responsive's sides stay in bounds wherever they come from", () => {
  for (const [w, h, ew, eh] of [
    [undefined, undefined, 400, 800],
    [0.4, 199.6, RESPONSIVE_MIN, 200],
    [-5, 1e9, RESPONSIVE_MIN, RESPONSIVE_MAX],
    [2999.5, 3000.4, RESPONSIVE_MAX, RESPONSIVE_MAX],
  ] as const) {
    const r = deviceOf({ name: RESPONSIVE, w, h })!;
    assert.deepEqual([r.w, r.h], [ew, eh], `${w}×${h}`);
  }
  // A stored choice past what a field or a drag gives is still a choice; deviceOf bounds it.
  for (const ok of [{ name: RESPONSIVE, w: 1e6, h: 0.5 }, { name: "" }, { name: "Gone Phone", rotated: false }]) assert.equal(isDeviceChoice(ok), true, JSON.stringify(ok));
  for (const no of [[], { name: RESPONSIVE, w: Infinity }, { name: RESPONSIVE, h: -0 }, { name: RESPONSIVE, w: 0 }, { name: "x", rotated: 1 }, { name: "x", w: null }]) {
    assert.equal(isDeviceChoice(no), false, JSON.stringify(no));
  }
});
