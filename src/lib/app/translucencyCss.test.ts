import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { contrast, opacityFloor } from "./opacityFloor.ts";

const SRC = join(import.meta.dirname, "../..");
const css = readFileSync(join(SRC, "index.css"), "utf8");
// Read, not imported: settings.ts wants a window and its storage.
const [min, max, step] = /WINDOW_OPACITY = \{ min: (\d+), max: (\d+), step: (\d+)/.exec(readFileSync(join(SRC, "lib/settings.ts"), "utf8"))!.slice(1).map(Number);
const WINDOW_OPACITY = { min, max, step };

function files(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? files(join(dir, e.name)) : e.name.endsWith(".tsx") ? [join(dir, e.name)] : [],
  );
}

// Inside a glass part its own color is transparent: a header pinned to the top in that color
// would let the rows scrolling under it show through, unless it's a .glass-header.
test("every header pinned to the top on a part's color is a glass-header", () => {
  const missing: string[] = [];
  for (const file of files(join(SRC, "features"))) {
    for (const m of readFileSync(file, "utf8").matchAll(/className="([^"]*)"/g)) {
      const cls = m[1].split(/\s+/);
      const pinned = cls.includes("sticky") && cls.some((c) => /^top-/.test(c));
      const covers = cls.some((c) => /^bg-(panel|sidebar|background|elevated)$/.test(c));
      if (pinned && covers && !cls.includes("glass-header")) missing.push(`${file.slice(SRC.length + 1)}: ${m[1]}`);
    }
  }
  assert.deepEqual(missing, []);
});

/** Each theme's colors as hex, the dark :root under every other theme. */
function themes() {
  const out: Record<string, Record<string, string>> = {};
  for (const m of css.matchAll(/:root(?:\[data-theme="([^"]+)"\])?\s*\{([^}]*)\}/g)) {
    const name = m[1] ?? "dark";
    if (out[name]) continue;
    const vars = Object.fromEntries([...m[2].matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})\b/g)].map((v) => [v[1], v[2]]));
    if (Object.keys(vars).length) out[name] = vars;
  }
  return Object.fromEntries(Object.entries(out).map(([k, v]) => [k, { ...out.dark, ...v }]));
}

const rgb = (h: string) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
// Composited as WebKit does, on the encoded values; the desktop's blur keeps its average.
const over = (c: number[], alpha: number, desk: number[]) => c.map((v, i) => v * alpha + desk[i] * (1 - alpha));

// The slider stops at each theme's floor (opacityFloor), and the page never goes below it.
test("at its floor, each theme's code text keeps 3:1 over a black or white desktop, and one step lower it wouldn't", () => {
  const low: string[] = [];
  for (const [name, v] of Object.entries(themes())) {
    const floor = opacityFloor(v.foreground, v.background, WINDOW_OPACITY);
    const ratios = (pct: number) => [[0, 0, 0], [1, 1, 1]].map((desk) => contrast(rgb(v.foreground), over(rgb(v.background), pct / 100, desk)));
    if (ratios(floor).some((r) => r < 3)) low.push(`${name} at ${floor}%: ${ratios(floor).map((r) => r.toFixed(2)).join(", ")}`);
    if (floor > WINDOW_OPACITY.min && ratios(floor - WINDOW_OPACITY.step).every((r) => r >= 3)) low.push(`${name}: ${floor}% isn't its lowest`);
  }
  assert.deepEqual(low, []);
  // The slider's own minimum isn't out of every theme's reach.
  assert.ok(Object.values(themes()).some((v) => opacityFloor(v.foreground, v.background, WINDOW_OPACITY) === WINDOW_OPACITY.min));
});
