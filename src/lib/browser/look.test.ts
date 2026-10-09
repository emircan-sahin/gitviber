import assert from "node:assert/strict";
import { test } from "node:test";
import { isScheme, isZoom, nextScheme, stepZoom, ZOOMS, zoomLabel } from "./look.ts";

test("zoom steps through Safari's stops and stays at either end", () => {
  assert.equal(stepZoom(1, 1), 1.15);
  assert.equal(stepZoom(1, -1), 0.85);
  assert.equal(stepZoom(3, 1), 3);
  assert.equal(stepZoom(0.5, -1), 0.5);
  assert.equal(stepZoom(2.5, 0), 1);
  // One between stops (an older build's) goes to the next stop that way.
  assert.equal(stepZoom(1.1, 1), 1.15);
  assert.equal(stepZoom(1.1, -1), 1);
  let z = 0.5;
  const seen = [z];
  while (stepZoom(z, 1) !== z) seen.push((z = stepZoom(z, 1)));
  assert.deepEqual(seen, ZOOMS);
  assert.equal(zoomLabel(0.85), "85%");
  assert.equal(zoomLabel(1.15), "115%");
});

test("saved zooms and schemes are read only when they're ones this build knows", () => {
  assert.ok(isZoom(1.25) && !isZoom(1.1) && !isZoom("1") && !isZoom(undefined));
  assert.ok(isScheme("dark") && !isScheme("auto") && !isScheme(undefined));
  assert.deepEqual([nextScheme(undefined), nextScheme("light"), nextScheme("dark")], ["light", "dark", undefined]);
});
