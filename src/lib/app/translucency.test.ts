import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// translucency.ts and settings.ts import "../api" and "./api" the bundler way (no extension, a
// folder): let node find the .ts.
// Each boot's ?boot= query is passed down to our own modules, so platform.ts is read afresh too.
register(
  "data:text/javascript," +
    encodeURIComponent(`
async function find(spec, ctx, next) {
  try {
    return await next(spec, ctx);
  } catch (e) {
    if (!spec.startsWith(".")) throw e;
    for (const tail of [".ts", "/index.ts"]) {
      try {
        return await next(spec + tail, ctx);
      } catch {}
    }
    throw e;
  }
}
export async function resolve(spec, ctx, next) {
  const found = await find(spec, ctx, next);
  const boot = ctx.parentURL && new URL(ctx.parentURL).search;
  if (boot && spec.startsWith(".") && !found.url.includes("?")) return { ...found, url: found.url + boot };
  return found;
}`),
);

type Settings = typeof import("../settings.ts");
// Not `typeof import`: that would type-check translucency.ts here, without Vite's import.meta.hot.
type Translucency = {
  glassLevels: (opacity: number) => { glass: string; chrome: string; step: string };
  previewTranslucency: (next: { windowOpacity?: number; backgroundBlur?: number }) => void;
};
type Ask = { on: boolean; blur: number };
type Pending = Ask & { resolve: () => void; reject: (e: unknown) => void };

/**
 * A fresh translucency.ts (and settings.ts) in a fake macOS window: set_translucent calls wait in `pending` (in the
 * order the main thread would run them) until drained, as a busy main thread would hold them.
 */
async function boot({
  stored,
  platform = "MacIntel",
  focused = false,
  fullscreen = false,
  reduce = false,
}: { stored?: object; platform?: string; focused?: boolean; fullscreen?: boolean; reduce?: boolean | (() => Promise<boolean>) } = {}) {
  const g = globalThis as Record<string, any>;
  Object.defineProperty(g, "navigator", { value: { platform, userAgent: "" }, configurable: true, writable: true });
  const store = new Map<string, string>();
  if (stored) store.set("gitviber.settings.v2", JSON.stringify(stored));
  g.localStorage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => store.set(k, v) };
  const style = new Map<string, string>();
  const root = { dataset: {} as Record<string, string>, style: { setProperty: (k: string, v: string) => style.set(k, v) } };
  g.document = { documentElement: root };
  // Each theme's text and background: Dark's floor is 55%, Light's 50% (opacityFloor).
  const colors: Record<string, Record<string, string>> = {
    dark: { "--foreground": "#ececee", "--background": "#171718" },
    light: { "--foreground": "#1d1d1f", "--background": "#ffffff" },
  };
  g.getComputedStyle = () => ({ getPropertyValue: (name: string) => colors[root.dataset.theme]?.[name] ?? "" });
  // index.css's palette rules, as appearanceFloor reads both themes' colors from them.
  g.CSSStyleRule = class {
    selectorText: string;
    style: { getPropertyValue: (name: string) => string };
    constructor(selectorText: string, style: { getPropertyValue: (name: string) => string }) {
      this.selectorText = selectorText;
      this.style = style;
    }
  };
  const rule = (selector: string, theme: string) => new g.CSSStyleRule(selector, { getPropertyValue: (name: string) => colors[theme][name] });
  (g.document as Record<string, unknown>).styleSheets = [{ cssRules: [rule(":root", "dark"), rule(':root[data-theme="light"]', "light")] }];
  g.window = g;
  g.location = { search: "" };
  g.matchMedia = () => ({ matches: true, addEventListener() {} });
  let frames: (() => void)[] = [];
  g.requestAnimationFrame = (cb: () => void) => frames.push(cb);

  const pending: Pending[] = [];
  const asked: Ask[] = [];
  let native: Ask = { on: false, blur: 0 };
  const callbacks = new Map<number, (e: unknown) => void>();
  const listeners = new Map<string, number[]>();
  let nextId = 1;
  g.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    transformCallback: (cb: (e: unknown) => void) => {
      callbacks.set(nextId, cb);
      return nextId++;
    },
    invoke: async (cmd: string, args: Record<string, any>) => {
      if (cmd === "set_translucent") {
        const ask = { on: args.on, blur: args.blur };
        asked.push(ask);
        return new Promise<void>((resolve, reject) =>
          pending.push({
            ...ask,
            resolve: () => {
              native = ask;
              resolve();
            },
            reject,
          }),
        );
      }
      if (cmd === "reduce_transparency") return typeof reduce === "function" ? reduce() : reduce;
      if (cmd === "plugin:window|is_focused") return focused;
      if (cmd === "plugin:window|is_fullscreen") return fullscreen;
      if (cmd === "plugin:event|listen") {
        listeners.set(args.event, [...(listeners.get(args.event) ?? []), args.handler]);
        return args.handler;
      }
      return null;
    },
  };

  const query = `?boot=${Math.random()}`;
  const translucency: Translucency = await import(`./translucency.ts${query}`);
  // The same URL as translucency.ts's own import: the same module.
  const settings: Settings = await import(`../settings.ts${query}`);
  await settle();

  async function settle() {
    for (let i = 0; i < 20; i++) await new Promise((r) => setImmediate(r));
  }
  async function fire(event: string, payload: unknown) {
    for (const id of listeners.get(event) ?? []) callbacks.get(id)!({ event, id: 0, payload });
    await settle();
  }
  return {
    settings,
    translucency,
    asked,
    pending,
    settle,
    get native() {
      return native;
    },
    /** The page's look: the code's opacity it shows, or undefined when solid. */
    get look() {
      return "translucency" in root.dataset ? style.get("--glass") : undefined;
    },
    style,
    /** Runs the main thread's queued set_translucent calls, in order, and the ones they lead to. */
    async drain() {
      // Lowering it asks Reduce transparency first, so the call can still be on its way.
      await settle();
      while (pending.length) {
        while (pending.length) pending.shift()!.resolve();
        await settle();
      }
    },
    async frame(n = 1) {
      for (let i = 0; i < n; i++) {
        const run = frames;
        frames = [];
        run.forEach((cb) => cb());
        await settle();
      }
    },
    fire,
    focus: (on: boolean) => fire(on ? "tauri://focus" : "tauri://blur", null),
    fullscreen: (on: boolean) => fire("fullscreen", on),
  };
}

const on = (blur: number) => ({ on: true, blur });
const off = { on: false, blur: 0 };

test("the levels: the code at the opacity, the bars a step more opaque, both solid at 100%", async () => {
  const { glassLevels } = (await boot()).translucency;
  assert.deepEqual(glassLevels(1), { glass: "100%", chrome: "100%", step: "25%" });
  assert.deepEqual(glassLevels(0.5), { glass: "50%", chrome: "62.5%", step: "25%" });
  assert.deepEqual(glassLevels(0.7), { glass: "70%", chrome: "77.5%", step: "25%" });
  // The code's tint and one step over it come to the bars' level: tabs and headers match them.
  for (let o = 0.5; o <= 1; o += 0.05) {
    const { glass, chrome, step } = glassLevels(o);
    const [g, c, s] = [glass, chrome, step].map((v) => parseFloat(v) / 100);
    assert.ok(Math.abs(1 - (1 - g) * (1 - s) - c) < 0.001, `at ${o}`);
    assert.ok(c >= g);
  }
});

test("at 100%, macOS asks nothing of the window and the page stays as it was", async () => {
  const w = await boot({ focused: true });
  await w.focus(false);
  await w.focus(true);
  w.settings.updateSettings({ appearance: "light", backgroundBlur: 30 });
  await w.frame(3);
  assert.equal(w.settings.getSettings().windowOpacity, 100);
  assert.deepEqual(w.asked, []);
  assert.equal(w.look, undefined);
  assert.deepEqual([...w.style.keys()].filter((k) => k.startsWith("--glass")), []);
});

test("Reduce transparency is asked only below 100%: once when lowered, then on each return", async () => {
  let asks = 0;
  const w = await boot({
    focused: true,
    reduce: async () => {
      asks++;
      return false;
    },
  });
  await w.focus(false);
  await w.focus(true);
  assert.equal(asks, 0, "solid");
  w.settings.updateSettings({ windowOpacity: 85 });
  await w.drain();
  assert.equal(asks, 1);
  w.settings.updateSettings({ windowOpacity: 70 });
  await w.focus(false);
  await w.focus(true);
  assert.equal(asks, 2);
});

test("lowered, the window goes clear with its blur before the page turns see-through", async () => {
  const w = await boot({ focused: true, stored: { backgroundBlur: 12 } });
  w.settings.updateSettings({ windowOpacity: 80 });
  await w.settle();
  assert.deepEqual(w.asked, [on(12)]);
  assert.equal(w.look, undefined, "solid before the window is clear");
  await w.drain();
  assert.deepEqual(w.native, on(12));
  assert.equal(w.look, "80%");
  assert.equal(w.style.get("--glass-chrome"), "85%");
});

test("the light and dark themes share the higher floor; the setting keeps its value", async () => {
  const w = await boot({ focused: true, stored: { windowOpacity: 50 } });
  await w.drain();
  assert.equal(w.look, "55%", "Dark stays readable down to 55%, Light to 50%: both stop at 55%");
  w.settings.updateSettings({ appearance: "light" });
  assert.equal(w.look, "55%", "the system's appearance switching doesn't move it");
  w.settings.updateSettings({ appearance: "dark" });
  assert.equal(w.look, "55%");
  assert.equal(w.settings.getSettings().windowOpacity, 50);
  assert.deepEqual(w.asked, [on(0)]);
});

test("a dragged opacity is previewed without saving, and the drop saves it", async () => {
  const w = await boot({ focused: true, stored: { windowOpacity: 90 } });
  await w.drain();
  const saved = () => JSON.parse(globalThis.localStorage.getItem("gitviber.settings.v2")!).windowOpacity;
  for (const windowOpacity of [85, 80, 75]) w.translucency.previewTranslucency({ windowOpacity });
  assert.equal(w.look, "75%");
  assert.equal(saved(), 90, "nothing saved while dragging");
  w.settings.updateSettings({ windowOpacity: 75 });
  assert.equal(w.look, "75%");
  assert.equal(saved(), 75);
});

test("more opacity changes, and theme switches, only restyle the page", async () => {
  const w = await boot({ focused: true });
  w.settings.updateSettings({ windowOpacity: 85 });
  await w.drain();
  for (const windowOpacity of [80, 75, 70, 50, 95]) w.settings.updateSettings({ windowOpacity });
  w.settings.updateSettings({ appearance: "dark", darkTheme: "dim" });
  w.settings.updateSettings({ appearance: "light" });
  await w.frame(3);
  assert.deepEqual(w.asked, [on(0)]);
  assert.equal(w.look, "95%");
});

test("a dragged blur goes to the window once a frame, the newest value", async () => {
  const w = await boot({ focused: true });
  w.settings.updateSettings({ windowOpacity: 70 });
  await w.drain();
  for (const backgroundBlur of [1, 2, 3, 4]) w.settings.updateSettings({ backgroundBlur });
  await w.frame(1);
  for (const backgroundBlur of [5, 6]) w.settings.updateSettings({ backgroundBlur });
  await w.frame(1);
  await w.frame(2);
  await w.drain();
  assert.deepEqual(w.asked, [on(0), on(4), on(6)]);
  assert.deepEqual(w.native, on(6));
});

test("a blur changed while the window is solid waits for it to go clear, and goes with it", async () => {
  const w = await boot({ focused: true });
  w.settings.updateSettings({ backgroundBlur: 25 });
  await w.frame(2);
  assert.deepEqual(w.asked, []);
  w.settings.updateSettings({ windowOpacity: 60 });
  await w.drain();
  await w.frame(2);
  assert.deepEqual(w.asked, [on(25)]);
});

test("a blur changed while the clear ask is on its way is sent once it's answered", async () => {
  const w = await boot({ focused: true });
  w.settings.updateSettings({ windowOpacity: 60 });
  await w.settle();
  w.settings.updateSettings({ backgroundBlur: 18 });
  await w.frame(1);
  assert.deepEqual(w.asked, [on(0)]);
  await w.drain();
  await w.frame(1);
  await w.drain();
  assert.deepEqual(w.asked, [on(0), on(18)]);
  assert.deepEqual(w.native, on(18));
});

test("back to 100%, the page goes solid at once and the window two frames later", async () => {
  const w = await boot({ focused: true });
  w.settings.updateSettings({ windowOpacity: 70 });
  await w.drain();
  w.settings.updateSettings({ windowOpacity: 100 });
  assert.equal(w.look, undefined);
  await w.frame(1);
  assert.deepEqual(w.asked, [on(0)], "still clear after one frame");
  await w.frame(1);
  await w.drain();
  assert.deepEqual(w.asked, [on(0), off]);
  assert.deepEqual(w.native, off);
});

test("down, up and down again within a frame ends with the window clear and the page see-through", async () => {
  const w = await boot({ focused: true });
  w.settings.updateSettings({ windowOpacity: 85 });
  w.settings.updateSettings({ windowOpacity: 100 });
  w.settings.updateSettings({ windowOpacity: 70 });
  await w.frame(3);
  await w.drain();
  assert.equal(w.asked.at(-1)!.on, true);
  assert.equal(w.native.on, true);
  assert.equal(w.look, "70%");
});

test("up, down and up again within a frame ends solid, the window too", async () => {
  const w = await boot({ focused: true, stored: { windowOpacity: 85 } });
  await w.drain();
  w.settings.updateSettings({ windowOpacity: 100 });
  w.settings.updateSettings({ windowOpacity: 85 });
  w.settings.updateSettings({ windowOpacity: 100 });
  await w.drain();
  assert.equal(w.look, undefined, "a late answer to the clear ask must not make the page see-through");
  await w.frame(3);
  await w.drain();
  assert.equal(w.native.on, false);
  assert.equal(w.look, undefined);
});

test("with the main thread held up, a stale answer never shows a see-through page over a solid window", async () => {
  const w = await boot({ focused: true });
  // 85, 100, 85 while the main thread runs something else: the first answer comes back after the
  // solid ask and the second clear one are already queued behind it.
  w.settings.updateSettings({ windowOpacity: 85 });
  // Past the Reduce transparency question, so the clear ask is sent.
  await w.settle();
  w.settings.updateSettings({ windowOpacity: 100 });
  await w.frame(2);
  w.settings.updateSettings({ windowOpacity: 85 });
  await w.settle();
  assert.deepEqual(
    w.pending.map((p) => p.on),
    [true, false, true],
  );
  while (w.pending.length) {
    w.pending.shift()!.resolve();
    await w.settle();
    if (w.look) assert.equal(w.native.on, true, `see-through page over a ${w.native.on ? "clear" : "solid"} window, ${w.pending.length} call(s) still queued`);
  }
  assert.equal(w.look, "85%");
});

test("dragging across 100% while the main thread is busy settles on the last value", async () => {
  const w = await boot({ focused: true });
  for (const windowOpacity of [95, 100, 70, 100, 85, 60, 100, 75]) {
    w.settings.updateSettings({ windowOpacity });
    await w.frame(1);
  }
  await w.frame(3);
  await w.drain();
  assert.equal(w.native.on, true);
  assert.equal(w.look, "75%");
  for (const windowOpacity of [100, 60, 100]) {
    w.settings.updateSettings({ windowOpacity });
    await w.frame(1);
  }
  await w.frame(3);
  await w.drain();
  assert.equal(w.native.on, false);
  assert.equal(w.look, undefined);
});

test("in the background the window stays see-through, and nothing is asked of it", async () => {
  const w = await boot({ focused: true, stored: { windowOpacity: 85 } });
  await w.drain();
  await w.focus(false);
  assert.equal(w.look, "85%");
  await w.focus(true);
  await w.drain();
  assert.equal(w.look, "85%");
  assert.deepEqual(w.asked, [on(0)]);
});

test("a focus storm ends the way the last answer says, whatever order Reduce transparency answers in", async () => {
  const answers: ((v: boolean) => void)[] = [];
  const w = await boot({ stored: { windowOpacity: 70 }, reduce: () => new Promise<boolean>((r) => answers.push(r)) });
  answers.shift()!(false);
  await w.drain();
  for (let i = 0; i < 10; i++) {
    await w.focus(true);
    await w.focus(false);
  }
  await w.focus(true);
  // The latest answer (Reduce transparency now on) first, the stale ones after.
  answers.pop()!(true);
  answers.splice(0).forEach((r) => r(false));
  await w.settle();
  assert.equal(w.look, undefined);
  await w.frame(2);
  await w.drain();
  assert.equal(w.native.on, false);
});

test("Reduce transparency turned on while away: solid on return, the window too, and clear again once it's off", async () => {
  let reduce = false;
  const w = await boot({ stored: { windowOpacity: 85 }, reduce: async () => reduce });
  await w.drain();
  assert.equal(w.look, "85%");
  await w.focus(false);
  reduce = true;
  await w.focus(true);
  assert.equal(w.look, undefined);
  await w.frame(2);
  await w.drain();
  assert.equal(w.native.on, false);
  await w.focus(false);
  reduce = false;
  await w.focus(true);
  await w.drain();
  assert.equal(w.native.on, true);
  assert.equal(w.look, "85%");
});

test("Reduce transparency switched while the window is in front is applied at once", async () => {
  let reduce = false;
  const w = await boot({ focused: true, stored: { windowOpacity: 85 }, reduce: async () => reduce });
  await w.drain();
  reduce = true;
  await w.fire("reduce-transparency", null);
  assert.equal(w.look, undefined);
  await w.frame(2);
  await w.drain();
  assert.equal(w.native.on, false);
  reduce = false;
  await w.fire("reduce-transparency", null);
  await w.drain();
  assert.equal(w.look, "85%");
});

test("Reduce transparency failing to answer still applies the setting", async () => {
  const w = await boot({ stored: { windowOpacity: 85 }, reduce: () => Promise.reject(new Error("gone")) });
  await w.drain();
  assert.equal(w.look, "85%");
});

test("launched below 100%: the window is asked once, and the page turns see-through when it's answered", async () => {
  const w = await boot({ stored: { windowOpacity: 70, backgroundBlur: 30 } });
  assert.deepEqual(w.asked, [on(30)]);
  assert.equal(w.look, undefined);
  await w.drain();
  assert.equal(w.look, "70%");
  assert.deepEqual(w.asked, [on(30)]);
});

test("full screen is solid, the window too, and see-through again on the way out", async () => {
  const w = await boot({ focused: true, stored: { windowOpacity: 70 } });
  await w.drain();
  await w.fullscreen(true);
  assert.equal(w.look, undefined);
  await w.frame(2);
  await w.drain();
  assert.deepEqual(w.native, off);
  w.settings.updateSettings({ windowOpacity: 60 });
  await w.frame(2);
  assert.equal(w.asked.length, 2, "nothing asked while in full screen");
  await w.fullscreen(false);
  await w.drain();
  assert.equal(w.native.on, true);
  assert.equal(w.look, "60%");
});

test("launched or reloaded in full screen, the window is never made clear", async () => {
  const w = await boot({ focused: true, fullscreen: true, stored: { windowOpacity: 70 } });
  await w.frame(3);
  await w.drain();
  assert.equal(w.look, undefined);
  assert.equal(w.native.on, false);
});

test("a set_translucent that fails leaves the page solid, and moving the slider through 100% retries", async () => {
  const w = await boot({ focused: true });
  w.settings.updateSettings({ windowOpacity: 85 });
  await w.settle();
  w.pending.shift()!.reject(new Error("no main thread"));
  await w.settle();
  assert.equal(w.look, undefined);
  w.settings.updateSettings({ windowOpacity: 100 });
  await w.frame(2);
  w.settings.updateSettings({ windowOpacity: 85 });
  await w.drain();
  assert.equal(w.look, "85%");
});

test("Translucency from before the sliders becomes an opacity and a blur", async () => {
  const cases = [
    ["off", 100, 0],
    ["subtle", 85, 20],
    ["strong", 70, 30],
  ] as const;
  for (const [translucency, windowOpacity, backgroundBlur] of cases) {
    const w = await boot({ stored: { appearance: "dark", translucency } });
    const s = w.settings.getSettings();
    assert.deepEqual([s.windowOpacity, s.backgroundBlur], [windowOpacity, backgroundBlur], translucency);
    assert.equal("translucency" in s, false);
  }
  // The sliders, once set, win over a leftover level.
  const w = await boot({ stored: { translucency: "strong", windowOpacity: 90, backgroundBlur: 4 } });
  assert.deepEqual([w.settings.getSettings().windowOpacity, w.settings.getSettings().backgroundBlur], [90, 4]);
});

test("stored values from older builds or hand edits fall back or snap into range", async () => {
  for (const bad of [undefined, null, true, "70", NaN, { v: 70 }, "__proto__"]) {
    const w = await boot({ stored: { translucency: bad, windowOpacity: bad, backgroundBlur: bad } });
    const s = w.settings.getSettings();
    assert.deepEqual([s.windowOpacity, s.backgroundBlur], [100, 0], `stored ${String(bad)}`);
    assert.deepEqual(w.asked, []);
  }
  const cases = [
    [10, -5, 50, 0],
    [72, 12.4, 70, 12],
    [73, 99, 75, 40],
    [140, 40, 100, 40],
  ];
  for (const [windowOpacity, backgroundBlur, opacityAfter, blurAfter] of cases) {
    const s = (await boot({ stored: { windowOpacity, backgroundBlur } })).settings.getSettings();
    assert.deepEqual([s.windowOpacity, s.backgroundBlur], [opacityAfter, blurAfter], `${windowOpacity}, ${backgroundBlur}`);
  }
});

test("Reset settings makes the window solid again", async () => {
  const w = await boot({ focused: true, stored: { windowOpacity: 70, backgroundBlur: 30 } });
  await w.drain();
  w.settings.resetSettings();
  await w.frame(3);
  await w.drain();
  assert.deepEqual([w.settings.getSettings().windowOpacity, w.settings.getSettings().backgroundBlur], [100, 0]);
  assert.deepEqual(w.native, off);
  assert.equal(w.look, undefined);
});

test("off macOS a stored opacity does nothing: no window calls, the page stays solid", async () => {
  for (const platform of ["Linux x86_64", "Win32"]) {
    const w = await boot({ platform, focused: true, stored: { windowOpacity: 70 } });
    w.settings.updateSettings({ windowOpacity: 60, backgroundBlur: 10 });
    await w.frame(3);
    assert.deepEqual(w.asked, [], platform);
    assert.equal(w.look, undefined, platform);
  }
});
