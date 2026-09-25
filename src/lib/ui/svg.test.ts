import assert from "node:assert/strict";
import { test } from "node:test";
import { panAxis, place, svgSize, zoomAxis, zoomLimits } from "./svg.ts";

test("an SVG sized only by its viewBox takes the viewBox size", () => {
  const icon = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" stroke-width="2"><path d="M0 0"/></svg>`;
  assert.deepEqual(svgSize(icon, [150, 150]), [24, 24]);
  assert.deepEqual(svgSize(`<svg viewBox="0,0,48,16">`, [300, 100]), [48, 16]);
  assert.deepEqual(svgSize(`<svg width="100%" height="100%" viewBox="0 0 10 20">`, [75, 150]), [10, 20]);
});

test("an absolute width and height win over the viewBox", () => {
  assert.deepEqual(svgSize(`<svg width="64" height="32" viewBox="0 0 24 24">`, [64, 32]), [64, 32]);
  assert.deepEqual(svgSize(`<?xml version="1.0"?>\n<svg\n  height="2em" width="10mm" viewBox="0 0 1 1">`, [38, 32]), [38, 32]);
});

test("falls back to the browser's size, then to 300×150", () => {
  assert.deepEqual(svgSize(`<svg><rect/></svg>`, [300, 150]), [300, 150]);
  assert.deepEqual(svgSize(`<svg viewBox="0 0 0 0">`, [0, 0]), [300, 150]);
  assert.deepEqual(svgSize(`not svg`, [0, 0]), [300, 150]);
});

test("an image that fits is centered, a bigger one never shows a gap", () => {
  assert.equal(place(100, 40, 0.1), 30);
  assert.equal(place(100, 400, 0.5), -150);
  assert.equal(place(100, 400, 0), 0);
  assert.equal(place(100, 400, 1), -300);
});

test("zooming keeps the point under the pointer in place", () => {
  // 400px image, centered on its middle, in a 100px panel: the pointer at 75 is over image px 225.
  const u = zoomAxis(100, 400, 800, 0.5, 75);
  assert.equal(place(100, 800, u) + 225 * 2, 75);
  // Zooming out until it fits centers it again.
  assert.equal(zoomAxis(100, 400, 80, 0.3, 10), 0.5);
});

test("panning moves the image with the pointer and stops at its edges", () => {
  const u = panAxis(100, 400, 0.5, 20);
  assert.equal(place(100, 400, u), -150 + 20);
  assert.equal(place(100, 400, panAxis(100, 400, 0.5, 1000)), 0);
  assert.equal(panAxis(100, 40, 0.5, 20), 0.5);
});

test("zoom limits reach 16× past fit or 1:1, but never draw past 16384px", () => {
  assert.deepEqual(zoomLimits([24, 24], 30), [0.5, 480]);
  assert.deepEqual(zoomLimits([2000, 1000], 0.4), [0.2, 8.192]);
});
