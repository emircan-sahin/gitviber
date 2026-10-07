import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

const SRC = join(import.meta.dirname, "../..");
const css = readFileSync(join(SRC, "index.css"), "utf8");
// Read, not imported: settings.ts wants a window and its storage.
const minOpacity = Number(/WINDOW_OPACITY = \{ min: (\d+)/.exec(readFileSync(join(SRC, "lib/settings.ts"), "utf8"))![1]);

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
const lum = (c: number[]) => {
  const l = c.map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2];
};
const contrast = (a: number[], b: number[]) => {
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};
// Composited as WebKit does, on the encoded values; the desktop's blur keeps its average.
const over = (c: number[], alpha: number, desk: number[]) => c.map((v, i) => v * alpha + desk[i] * (1 - alpha));

// WINDOW_OPACITY's note measured this for the dark theme over a white desktop only. A light theme
// over a dark desktop, or a darker dark theme over a white one, comes out lower.
test("at the lowest opacity, the code's text keeps 3:1 in every theme over a black or white desktop", { todo: "the 50% floor holds for the default dark theme only" }, () => {
  // The code's level is the opacity itself (glassLevels).
  const code = minOpacity / 100;
  const low: string[] = [];
  for (const [name, v] of Object.entries(themes())) {
    for (const [desk, under] of [["black", [0, 0, 0]], ["white", [1, 1, 1]]] as const) {
      const ratio = contrast(rgb(v.foreground), over(rgb(v.background), code, [...under]));
      if (ratio < 3) low.push(`${name} on ${desk}: ${ratio.toFixed(2)}:1`);
    }
  }
  assert.deepEqual(low, []);
});
